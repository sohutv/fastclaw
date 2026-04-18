use super::{TaskEnabled, TaskInfo, TaskRunState};
use crate::channels::{Anonymous, SessionId};
use crate::tools::task_tool::detail::TaskDetailGetTool;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use anyhow::anyhow;
use itertools::Itertools;
use log::error;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use sqlx::sqlite::SqliteArguments;
use sqlx::{Arguments, QueryBuilder, Sqlite};
use strum::IntoEnumIterator;
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct TaskListTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    run_state: Option<TaskRunState>,
    enabled: TaskEnabled,
    session_id: Anonymous,
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
                    "session_id": {
                        "type": "string",
                        "description": "The current session-id",
                    },
                },
                "required": ["enabled", "session_id"],
            }),
        }
    }
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let session_id = SessionId::from(&args.session_id);
        let sql_pool = self
            .ctx
            .agent_context
            .workspace
            .sql_pool(&session_id)
            .await
            .map_err(|err| ToolCallError(format!("fail to get sql_pool, err: {err}")))?;
        let mut query_builder = args
            .create_sql()
            .map_err(|err| ToolCallError(format!("{err}")))?;
        let mut stream = query_builder.build().fetch(&*sql_pool);
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
- **last_exe_at**: {},
"#,
                    task.name,
                    task.id,
                    task.task_schedule,
                    task.session_id,
                    task.run_state,
                    task.enabled,
                    task.last_exe_at
                        .map(|it| it.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "never".to_string()),
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
