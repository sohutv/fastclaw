use crate::config::Workspace;
use crate::tools::ToolCallError;
use crate::tools::task_tool::{CREATE_TASK_TABLE, TaskEnabled, TaskInfo, TaskRunState, TaskTools};
use anyhow::anyhow;
use itertools::Itertools;

impl TaskTools {
    pub async fn init_cron_task(workspace: &Workspace) -> crate::Result<()> {
        let _ = sqlx::query(CREATE_TASK_TABLE)
            .execute(&workspace.sql_pool)
            .await
            .map_err(|err| ToolCallError(format!("{err}")))?;
        Ok(())
    }

    pub async fn fetch_ready_tasks(workspace: &Workspace) -> crate::Result<Vec<TaskInfo>> {
        let mut query_builder =
            sqlx::QueryBuilder::new("select * from cron_task where deleted = 0 ");
        let tasks = query_builder
            .push("and enabled = ")
            .push_bind(TaskEnabled::Enabled as u8)
            .push("and run_state = ")
            .push_bind(TaskRunState::Ready as u16)
            .build()
            .fetch_all(&workspace.sql_pool)
            .await
            .map_err(|err| anyhow!(err))?
            .into_iter()
            .flat_map(|row| TaskInfo::try_from(row))
            .collect_vec();
        Ok(tasks)
    }
}
