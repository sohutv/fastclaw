use crate::channels::SessionId;
use crate::config::Workspace;
use crate::tools::task_tool::{CREATE_TASK_TABLE, TaskEnabled, TaskInfo, TaskRunState, TaskTools};
use crate::tools::{TaskSchedule, ToolCallError};
use anyhow::anyhow;
use itertools::Itertools;
use sqlx::SqlitePool;

impl TaskTools {
    pub async fn init_cron_task(sql_pool: &SqlitePool) -> crate::Result<()> {
        let _ = sqlx::query(CREATE_TASK_TABLE)
            .execute(&*sql_pool)
            .await
            .map_err(|err| ToolCallError(format!("{err}")))?;
        Ok(())
    }

    pub async fn fetch_ready_tasks(
        workspace: &Workspace,
        session_id: &SessionId,
    ) -> crate::Result<Vec<TaskInfo>> {
        let sql_pool = workspace.sql_pool(session_id).await?;
        let mut query_builder =
            sqlx::QueryBuilder::new("select * from cron_task where deleted = 0 ");
        let tasks = query_builder
            .push("and enabled = ")
            .push_bind(TaskEnabled::Enabled as u8)
            .push("and run_state = ")
            .push_bind(TaskRunState::Ready as u16)
            .build()
            .fetch_all(&*sql_pool)
            .await
            .map_err(|err| anyhow!(err))?
            .into_iter()
            .flat_map(|row| TaskInfo::try_from(row))
            .collect_vec();
        Ok(tasks)
    }

    pub async fn mark_task_executed(
        workspace: &Workspace,
        session_id: &SessionId,
        task_id: u64,
        task_schedule: &TaskSchedule,
    ) -> crate::Result<()> {
        let sql_pool = workspace.sql_pool(session_id).await?;
        let task_id =
            i64::try_from(task_id).map_err(|_| anyhow!("task id {} is out of range", task_id))?;
        sqlx::query(
            "update `cron_task` set `last_exe_at` = CURRENT_TIMESTAMP, `updated_at` = CURRENT_TIMESTAMP where `id` = ? and `deleted` = ?",
        )
        .bind(task_id).bind(match task_schedule{
            TaskSchedule::Cron(_) => 0,
            TaskSchedule::Datetime(_) => 1
        })
        .execute(&*sql_pool)
        .await
        .map_err(|err| anyhow!(err))?;
        Ok(())
    }
}
