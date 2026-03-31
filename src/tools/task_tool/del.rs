use crate::agent::AgentContext;
use crate::tools::{ToolCallError, ToolCallRsult};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use std::sync::Arc;
use log::error;

#[derive(Clone)]
pub struct TaskDelTool {
    ctx: Arc<AgentContext>,
}

impl TaskDelTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    id: u64,
}

#[allow(async_fn_in_trait)]
impl Tool for TaskDelTool {
    const NAME: &'static str = "task-del";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Delete one task by id".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The id of task to delete",
                    }
                },
                "required": ["id"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let task_id = i64::try_from(args.id)
            .map_err(|_| ToolCallError(format!("task id {} is out of range", args.id)))?;
        let result =
            sqlx::query("update `cron_task` set `deleted` = 1, `updated_at` = CURRENT_TIMESTAMP where `id` = ? and `deleted` = 0")
                .bind(task_id)
                .execute(&self.ctx.workspace.sql_pool)
                .await
                .map_err(|err| {
                    error!("{err}");
                    ToolCallError(format!("{err}"))
                })?;
        if result.rows_affected() == 0 {
            return Ok(ToolCallRsult::error(format!("task {} not found", args.id)));
        }
        Ok(ToolCallRsult::ok(format!("delete task {} ok", args.id)))
    }
}
