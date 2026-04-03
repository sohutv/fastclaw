use crate::config::Workspace;
use crate::tools::ToolCallError;
use crate::tools::task_tool::{CREATE_TASK_TABLE, TaskEnabled, TaskInfo, TaskRunState, TaskTools};
use anyhow::anyhow;
use itertools::Itertools;
use log::warn;

impl TaskTools {
    pub async fn init_cron_task(workspace: &Workspace) -> crate::Result<()> {
        let _ = sqlx::query(CREATE_TASK_TABLE)
            .execute(&workspace.sql_pool)
            .await
            .map_err(|err| ToolCallError(format!("{err}")))?;
        if let Err(err) = sqlx::query("alter table `cron_task` add column `last_exe_at` TIMESTAMP")
            .execute(&workspace.sql_pool)
            .await
        {
            let err_msg = err.to_string();
            if !err_msg.contains("duplicate column name") {
                return Err(ToolCallError(format!("{err}")).into());
            }
            warn!("cron_task.last_exe_at already exists");
        }
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

    pub async fn mark_task_executed(workspace: &Workspace, task_id: u64) -> crate::Result<()> {
        let task_id =
            i64::try_from(task_id).map_err(|_| anyhow!("task id {} is out of range", task_id))?;
        sqlx::query(
            "update `cron_task` set `last_exe_at` = CURRENT_TIMESTAMP, `updated_at` = CURRENT_TIMESTAMP where `id` = ? and `deleted` = 0",
        )
        .bind(task_id)
        .execute(&workspace.sql_pool)
        .await
        .map_err(|err| anyhow!(err))?;
        Ok(())
    }
}
