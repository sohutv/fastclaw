use crate::tools::ToolContext;
use anyhow::anyhow;
use derive_more::Display;
use rig::tool::ToolDyn;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use std::str::FromStr;
use strum::{EnumIter, IntoEnumIterator};

mod create;
mod del;
mod detail;
mod list;
mod update;

pub mod task_api;

const DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

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
    pub task_schedule: TaskSchedule,
    pub desc: String,
    pub session_id: String,
    pub run_state: TaskRunState,
    pub enabled: TaskEnabled,
    pub created_at: Datetime,
    pub updated_at: Datetime,
    pub last_exe_at: Option<Datetime>,
    pub creator: String,
}

mod task_schedule;
pub use task_schedule::TaskSchedule;

mod datetime;
pub use datetime::Datetime;

impl TryFrom<SqliteRow> for TaskInfo {
    type Error = anyhow::Error;

    fn try_from(row: SqliteRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            task_schedule: {
                let s: &str = row.try_get("cron")?;
                TaskSchedule::from_str(s)?
            },
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
                Datetime::from_str(&ts)?
            },
            updated_at: {
                let ts: String = row.try_get("updated_at")?;
                Datetime::from_str(&ts)?
            },
            last_exe_at: {
                let ts: Option<String> = row.try_get("last_exe_at")?;
                ts.map(|ts| Datetime::from_str(&ts)).transpose()?
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

impl TaskInfo {
    pub fn brief_desc(&self) -> String {
        format!(
            r#"
## {}
- **id**: {},
- **cron**: {},
- **session_id**: {},
- **run_state**: {},
- **enabled**: {},
- **last_exe_at**: {},
"#,
            self.name,
            self.id,
            self.task_schedule,
            self.session_id,
            self.run_state,
            self.enabled,
            self.last_exe_at
                .as_ref()
                .map(|it| it.to_string())
                .unwrap_or_else(|| "never".to_string()),
        )
    }
    pub fn full_desc(&self) -> String {
        format!(
            r#"
## {}
- **id**: {},
- **cron**: {},
- **session_id**: {},
- **run_state**: {},
- **enabled**: {},
- **created_at**: {},
- **updated_at**: {},
- **last_exe_at**: {},
- **creator**: {}
- **desc**:
```
{}
```                    "#,
            self.name,
            self.id,
            self.task_schedule,
            self.session_id,
            self.run_state,
            self.enabled,
            self.created_at,
            self.updated_at,
            self.last_exe_at
                .as_ref()
                .map(|it| it.to_string())
                .unwrap_or_else(|| "never".to_string()),
            self.creator,
            self.desc,
        )
    }
}
