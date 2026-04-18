use crate::tools::ToolContext;
use anyhow::anyhow;
use chrono::Local;
use derive_more::Display;
use rig::tool::ToolDyn;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use strum::{EnumIter, IntoEnumIterator};

mod create;
mod del;
mod detail;
mod list;
mod update;

pub mod task_api;

#[derive(Clone)]
pub struct TaskTools;

impl TaskTools {
    pub async fn create(ctx: ToolContext) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        Ok(vec![
            Box::new(list::TaskListTool { ctx: ctx.clone() }),
            Box::new(create::TaskCreateTool { ctx: ctx.clone() }),
            Box::new(detail::TaskDetailGetTool { ctx: ctx.clone() }),
            Box::new(update::TaskUpdateTool { ctx: ctx.clone() }),
            Box::new(del::TaskDelTool { ctx: ctx.clone() }),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub id: u64,
    pub name: String,
    pub cron: String,
    pub desc: String,
    pub session_id: String,
    pub run_state: TaskRunState,
    pub enabled: TaskEnabled,
    pub created_at: chrono::DateTime<Local>,
    pub updated_at: chrono::DateTime<Local>,
    pub last_exe_at: Option<chrono::DateTime<Local>>,
    pub creator: String,
}

impl TryFrom<SqliteRow> for TaskInfo {
    type Error = anyhow::Error;

    fn try_from(row: SqliteRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            cron: row.try_get("cron")?,
            desc: row.try_get("desc")?,
            session_id: row.try_get("session_id")?,
            run_state: {
                let val: u16 = row.try_get("run_state")?;
                TaskRunState::iter()
                    .find(|&state| state as u16 == val)
                    .ok_or(anyhow!("Invalid run state value: {}", val))?
            },
            enabled: {
                let val: u8 = row.try_get("enabled")?;
                TaskEnabled::iter()
                    .find(|&state| state as u8 == val)
                    .ok_or(anyhow!("Invalid enabled value: {}", val))?
            },
            created_at: {
                let ts: String = row.try_get("created_at")?;
                chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%d %H:%M:%S")?
                    .and_utc()
                    .with_timezone(&Local)
            },
            updated_at: {
                let ts: String = row.try_get("updated_at")?;
                chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%d %H:%M:%S")?
                    .and_utc()
                    .with_timezone(&Local)
            },
            last_exe_at: {
                let ts: Option<String> = row.try_get("last_exe_at")?;
                ts.map(|ts| {
                    chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%d %H:%M:%S")
                        .map(|dt| dt.and_utc().with_timezone(&Local))
                })
                .transpose()?
            },
            creator: row.try_get("creator")?,
        })
    }
}

#[derive(Debug, Clone, Copy, serde::Deserialize, EnumIter, Display)]
#[repr(u16)]
pub enum TaskRunState {
    Ready = 0x01,
    Running = 0x10,
    Completed = 0x100,
    Failed = 0x101,
}
#[derive(Debug, Clone, Copy, serde::Deserialize, EnumIter, Display)]
#[repr(u8)]
pub enum TaskEnabled {
    Enabled = 1,
    Disabled = 0,
}

const CREATE_TASK_TABLE: &str = r#"
create table if not exists cron_task
(

    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,
    cron         TEXT NOT NULL,
    desc         TEXT NOT NULL,
    session_id   TEXT,
    run_state    INTEGER,
    enabled      INTEGER   DEFAULT 1,
    created_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_exe_at  TIMESTAMP,
    deleted      INTEGER   DEFAULT 0,
    creator      TEXT NOT NULL
);
"#;
