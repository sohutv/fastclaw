use super::TaskInfo;
use crate::agent::AgentContext;
use crate::tools::{ToolCallError, ToolCallRsult};
use log::error;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct TaskDetailGetTool {
    ctx: Arc<AgentContext>,
}

impl TaskDetailGetTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
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
                    }
                },
                "required": ["id"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let task_id = i64::try_from(args.id)
            .map_err(|_| ToolCallError(format!("task id {} is out of range", args.id)))?;
        let row = sqlx::query("select * from `cron_task` where `deleted` = 0 and `id` = ? ")
            .bind(task_id)
            .fetch_optional(&self.ctx.workspace.sql_pool)
            .await
            .map_err(|err| {
                error!("{err}");
                ToolCallError(format!("{err}"))
            })?;
        let Some(row) = row else {
            return Ok(ToolCallRsult::error(format!("task {} not found", args.id)));
        };
        let task = TaskInfo::try_from(row).map_err(|err| ToolCallError(format!("{err}")))?;
        Ok(ToolCallRsult::ok(format!(
            r#"
## {}
- **id**: {},
- **cron**: {},
- **session_id**: {},
- **run_state**: {},
- **enabled**: {},
- **created_at**: {},
- **updated_at**: {},
- **creator**: {}
- **desc**:
```
{}
```                    "#,
            task.name,
            task.id,
            task.cron,
            task.session_id,
            task.run_state,
            task.enabled,
            task.created_at.format("%Y-%m-%d %H:%M:%S"),
            task.updated_at.format("%Y-%m-%d %H:%M:%S"),
            task.creator,
            task.desc,
        )))
    }
}
