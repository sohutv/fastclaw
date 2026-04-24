use super::TaskInfo;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use log::error;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub struct TaskDetailGetTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    id: u64,
}

#[allow(async_fn_in_trait)]
impl Tool for TaskDetailGetTool {
    const NAME: &'static str = "task-detail-get";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get detail for one task by id".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The task id",
                    },
                },
                "required": ["id"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let sql_pool = self
            .ctx
            .agent_context()
            .workspace
            .sql_pool(&self.ctx.session_id)
            .await
            .map_err(|err| ToolCallError(format!("fail to get sql_pool, err: {err}")))?;
        let task_id = i64::try_from(args.id)
            .map_err(|_| ToolCallError(format!("task id {} is out of range", args.id)))?;
        let row = sqlx::query("select * from `cron_task` where `deleted` = 0 and `id` = ? ")
            .bind(task_id)
            .fetch_optional(&*sql_pool)
            .await
            .map_err(|err| {
                error!("{err}");
                ToolCallError(format!("{err}"))
            })?;
        let Some(row) = row else {
            return Ok(ToolCallRsult::error(format!("task {} not found", args.id)));
        };
        let task = TaskInfo::try_from(row).map_err(|err| ToolCallError(format!("{err}")))?;
        Ok(ToolCallRsult::ok(task.full_desc()))
    }
}
