use super::TaskEnabled;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use log::error;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use sqlx::{QueryBuilder, Sqlite};
use strum::IntoEnumIterator;

#[derive(Clone)]
pub struct TaskUpdateTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    id: u64,
    name: Option<String>,
    cron: Option<String>,
    desc: Option<String>,
    enabled: Option<TaskEnabled>,
}

#[allow(async_fn_in_trait)]
impl Tool for TaskUpdateTool {
    const NAME: &'static str = "task-update";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Update one task by id".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "integer",
                        "description": "The id of task to update",
                    },
                    "name": {
                        "type": "string",
                        "description": "The name of the task",
                    },
                    "cron": {
                        "type": "string",
                        "description": "The cron expression for the task",
                    },
                    "desc": {
                        "type": "string",
                        "description": "The description of the task",
                    },
                    "enabled": {
                        "type": "enum",
                        "enum": TaskEnabled::iter().map(|it| it.to_string()).collect::<Vec<_>>(),
                        "description": "The enabled state of task",
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
        let Some(mut query_builder) = args.create_update_sql(task_id) else {
            return Ok(ToolCallRsult::error(
                "no fields to update, provide at least one optional field",
            ));
        };
        let result = query_builder
            .build()
            .execute(&*sql_pool)
            .await
            .map_err(|err| {
                error!("{err}");
                ToolCallError(format!("{err}"))
            })?;
        if result.rows_affected() == 0 {
            return Ok(ToolCallRsult::error(format!("task {} not found", args.id)));
        }
        Ok(ToolCallRsult::ok(format!("update task {} ok", args.id)))
    }
}

impl Args {
    fn create_update_sql(&self, task_id: i64) -> Option<QueryBuilder<'_, Sqlite>> {
        let mut query_builder = QueryBuilder::new("update `cron_task` set ");
        let mut has_update = false;
        if let Some(name) = &self.name {
            has_update = true;
            query_builder.push("`name` = ").push_bind(name).push(", ");
        }
        if let Some(cron) = &self.cron {
            has_update = true;
            query_builder.push("`cron` = ").push_bind(cron).push(", ");
        }
        if let Some(desc) = &self.desc {
            has_update = true;
            query_builder.push("`desc` = ").push_bind(desc).push(", ");
        }
        if let Some(enabled) = self.enabled {
            has_update = true;
            query_builder
                .push("`enabled` = ")
                .push_bind(enabled as u8)
                .push(", ");
        }
        if !has_update {
            return None;
        }
        query_builder
            .push("`updated_at` = CURRENT_TIMESTAMP")
            .push(" where `id` = ")
            .push_bind(task_id);
        Some(query_builder)
    }
}
