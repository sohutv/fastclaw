use crate::agent::AgentContext;
use crate::channels::SessionId;
use crate::tools::{ToolCallError, ToolCallRsult};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct TaskCreateTool {
    ctx: Arc<AgentContext>,
}

impl TaskCreateTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TaskCreateArgs {
    name: String,
    cron: String,
    desc: String,
    session_id: String,
    agent_id: String,
}

#[allow(async_fn_in_trait)]
impl Tool for TaskCreateTool {
    const NAME: &'static str = "task-create";
    type Error = ToolCallError;
    type Args = TaskCreateArgs;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Create Task with description".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The name of the task to create",
                    },
                    "cron": {
                        "type": "string",
                        "description": "The cron expression for the task schedule",
                    },
                    "desc": {
                        "type": "string",
                        "description": "The description of the task",
                    },
                    "session_id": {
                        "type": "string",
                        "description": "The current session-id",
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "The current agent-id",
                    },
                },
                "required": ["name","cron", "desc","session_id","agent_id"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = sqlx::query(CREATE_TASK_TABLE)
            .execute(&self.ctx.workspace.sql_pool)
            .await
            .map_err(|err| ToolCallError(format!("{err}")))?;
        let result = sqlx::query(&args.create_sql())
            .execute(&self.ctx.workspace.sql_pool)
            .await
            .map_err(|err| ToolCallError(format!("{err}")))?;
        Ok(ToolCallRsult::ok(format!("create task {} ok", args.name)))
    }
}

#[derive(Debug, Clone)]
pub struct TaskInfo {
    id: u64,
    name: String,
    cron: String,
    desc: String,
    session_id: SessionId,
    run_state: TaskRunState,
    enabled: TaskEnabled,
    created_at: chrono::DateTime<chrono::Local>,
    updated_at: chrono::DateTime<chrono::Local>,
    creator: String,
}

impl TaskInfo {}

impl TaskCreateArgs {
    fn create_sql(&self) -> String {
        let Self {
            name,
            cron,
            desc,
            session_id,
            agent_id,
            ..
        } = self;
        format!(
            r#"
insert into `cron_task`(`name`, `cron`, `desc`, `session_id`, `run_state`, `enabled`, `creator`)
values ('{}', '{}', '{}', '{}', '{}', {}, {})
;
        "#,
            name,
            cron,
            desc,
            session_id,
            TaskRunState::Ready as u16,
            TaskEnabled::Enabled as u8,
            agent_id
        )
    }
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[repr(u16)]
pub enum TaskRunState {
    Ready = 0x01,
    Running = 0x10,
    Completed = 0x100,
    Failed = 0x101,
}
#[derive(Debug, Clone, Copy, serde::Deserialize)]
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
    creator      TEXT NOT NULL
);
"#;
