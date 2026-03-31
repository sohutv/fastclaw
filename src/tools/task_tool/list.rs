use super::{TaskEnabled, TaskInfo, TaskRunState};
use crate::agent::AgentContext;
use crate::tools::task_tool::detail::TaskDetailGetTool;
use crate::tools::{ToolCallError, ToolCallRsult};
use anyhow::anyhow;
use itertools::Itertools;
use log::error;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use sqlx::sqlite::SqliteArguments;
use sqlx::{Arguments, QueryBuilder, Sqlite};
use std::sync::Arc;
use strum::IntoEnumIterator;
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct TaskListTool {
    ctx: Arc<AgentContext>,
}

impl TaskListTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    run_state: Option<TaskRunState>,
    enabled: TaskEnabled,
}

#[allow(async_fn_in_trait)]
impl Tool for TaskListTool {
    const NAME: &'static str = "task-list";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List the specified tasks".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "run_state": {
                        "type": "enum",
                        "enum": TaskRunState::iter().map(|it|it.to_string()).collect::<Vec<_>>(),
                        "description": "The run state of the task",
                    },
                    "enabled": {
                        "type": "enum",
                        "enum": TaskEnabled::iter().map(|it|it.to_string()).collect::<Vec<_>>(),
                        "description": "The enabled state of the task",
                    },
                },
                "required": ["enabled"],
            }),
        }
    }
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut query_builder = args
            .create_sql()
            .map_err(|err| ToolCallError(format!("{err}")))?;
        let mut stream = query_builder.build().fetch(&self.ctx.workspace.sql_pool);
        let mut tasks = vec![];
        while let Some(row) = stream.next().await {
            if let Ok(row) = row {
                let task = TaskInfo::try_from(row).map_err(|err| {
                    error!("{err}");
                    ToolCallError(format!("{err}"))
                })?;
                tasks.push(format!(
                    r#"
## {}
- **id**: {},
- **cron**: {},
- **session_id**: {},
- **run_state**: {},
- **enabled**: {},
"#,
                    task.name, task.id, task.cron, task.session_id, task.run_state, task.enabled,
                ));
            }
        }
        let output = format!(
            r#"
{}

**Tips**: Call {} for task detail
        "#,
            tasks.iter().join("\n\n"),
            TaskDetailGetTool::NAME
        );
        Ok(ToolCallRsult::ok(output))
    }
}

impl Args {
    fn create_sql(&self) -> crate::Result<QueryBuilder<'_, Sqlite>> {
        let mut args = SqliteArguments::default();
        let sql = if let Some(run_state) = self.run_state {
            args.add(self.enabled as u8).map_err(|err| anyhow!(err))?;
            args.add(run_state as u16).map_err(|err| anyhow!(err))?;
            "select * from `cron_task` where `deleted` = 0 and `enabled` = ? and `run_state` = ?"
        } else {
            args.add(self.enabled as u8).map_err(|err| anyhow!(err))?;
            "select * from `cron_task` where `deleted` = 0 and `enabled` = ?"
        };
        Ok(QueryBuilder::with_arguments(sql, args))
    }
}
