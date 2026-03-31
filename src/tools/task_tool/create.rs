use crate::agent::AgentContext;
use crate::tools::task_tool::{TaskEnabled, TaskRunState};
use crate::tools::{ToolCallError, ToolCallRsult};
use log::error;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use sqlx::{QueryBuilder, Sqlite};
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
pub struct Args {
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
    type Args = Args;
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
        let mut query_builder = args.create_sql();
        let _ = query_builder
            .build()
            .execute(&self.ctx.workspace.sql_pool)
            .await
            .map_err(|err| {
                error!("{err}");
                ToolCallError(format!("{err}"))
            })?;
        Ok(ToolCallRsult::ok(format!("create task {} ok", args.name)))
    }
}

impl Args {
    fn create_sql(&self) -> QueryBuilder<'_, Sqlite> {
        let Self {
            name,
            cron,
            desc,
            session_id,
            agent_id,
            ..
        } = self;
        let mut builder = QueryBuilder::new(
            "insert into `cron_task`(`name`, `cron`, `desc`, `session_id`, `run_state`, `enabled`, `creator`) values ",
        );
        builder
            .push("(")
            .push_bind(name)
            .push(", ")
            .push_bind(cron)
            .push(", ")
            .push_bind(desc)
            .push(", ")
            .push_bind(session_id)
            .push(", ")
            .push_bind(TaskRunState::Ready as u16)
            .push(", ")
            .push_bind(TaskEnabled::Enabled as u8)
            .push(", ")
            .push_bind(agent_id)
            .push(")");
        builder
    }
}
