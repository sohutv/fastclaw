use crate::channels::SessionId;
use crate::tools::task_tool::{DATETIME_FORMAT, TaskEnabled, TaskRunState};
use crate::tools::{TaskSchedule, ToolCallError, ToolCallRsult, ToolContext};
use log::error;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use sqlx::{QueryBuilder, Sqlite};

#[derive(Clone)]
pub struct TaskCreateTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    name: String,
    task_schedule: TaskSchedule,
    desc: String,
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
                    "task_schedule": {
                        "type": "string",
                        "description": format!(r#"
### Task scheduling configuration supports the following formats:
- 1 **Cyclic timed scheduling task**: The standard task_schedule expression for the task schedule, e.g. `0 0 9-18 * * * ?` .
- 2 **Single scheduling task**: Specify the absolute trigger time {DATETIME_FORMAT} , e.g. `2026-04-18 14:00:00`
                        "#),
                    },
                    "desc": {
                        "type": "string",
                        "description": "The description of the task",
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "The current agent-id",
                    },
                },
                "required": ["name","task_schedule", "desc","agent_id"],
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
        let mut query_builder = args.create_sql(&self.ctx.session_id);
        let _ = query_builder
            .build()
            .execute(&*sql_pool)
            .await
            .map_err(|err| {
                error!("{err}");
                ToolCallError(format!("{err}"))
            })?;
        Ok(ToolCallRsult::ok(format!("create task {} ok", args.name)))
    }
}

impl Args {
    fn create_sql(&self, session_id: &SessionId) -> QueryBuilder<'_, Sqlite> {
        let Self {
            name,
            task_schedule,
            desc,
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
            .push_bind(task_schedule.to_string())
            .push(", ")
            .push_bind(desc)
            .push(", ")
            .push_bind(session_id.to_string())
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
