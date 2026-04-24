use crate::agent::AgentRequest;
use crate::channels::{Anonymous, Channel, SessionId};
use crate::tools::{TaskSchedule, TaskTools};
use chrono::Duration;
use log::{error, info, warn};
use rig::completion::Message;
use std::str::FromStr;
use std::sync::Arc;

impl<C, Client> super::Heartbeat<C, Client>
where
    C: Channel<Client = Client>,
{
    pub(super) async fn spawn_cron_tasks(&self) -> crate::Result<()> {
        let session_ids = self.channel.allow_session_ids()?;
        for session_id in session_ids {
            match self.spawn_cron_tasks_actual(session_id).await {
                Ok(_) => {}
                Err(err) => {
                    warn!("spawn_cron_tasks failed, err: {err}");
                }
            }
        }
        Ok(())
    }
    async fn spawn_cron_tasks_actual(&self, session_id: &SessionId) -> crate::Result<()> {
        let tasks = TaskTools::fetch_ready_tasks(&self.workspace, session_id).await?;
        let now = chrono::Local::now();
        let (down, up) = (now - Duration::hours(1), now);
        for task in tasks {
            // Parse cron expression and check if current time matches
            let time_to_exec = match &task.task_schedule {
                TaskSchedule::Cron(cron) => match cron::Schedule::from_str(&cron) {
                    Ok(schedule) => {
                        let last_exe_at = task.last_exe_at.as_ref().unwrap_or(&task.created_at);
                        if let Some(next) = schedule.after(last_exe_at).next() {
                            down < next && next < up
                        } else {
                            false
                        }
                    }
                    Err(err) => {
                        error!(
                            "Failed to parse cron expression '{}' for task {}: {}",
                            cron, &task.id, err
                        );
                        false
                    }
                },
                TaskSchedule::Datetime(dt) => {
                    if let (Some(dt), None) = (
                        dt.and_local_timezone(now.timezone()).single(),
                        &task.last_exe_at,
                    ) {
                        down < dt && dt < up
                    } else {
                        false
                    }
                }
            };
            if time_to_exec {
                let session_id = SessionId::Anonymous {
                    val: Anonymous(task.session_id.clone()),
                    settings: Default::default(),
                };
                // To optimize the problem of repeated task execution, the task status will be marked first.
                if let Err(err) = TaskTools::mark_task_executed(
                    &self.workspace,
                    &session_id,
                    task.id,
                    &task.task_schedule,
                )
                .await
                {
                    error!(
                        "Failed to update last_exe_at for task '{}' (id: {}): {}",
                        task.name, task.id, err
                    );
                }
                match Arc::clone(&self.channel)
                    .submit_agent_task(
                        Arc::clone(&self.client),
                        Arc::clone(&self.agent),
                        None,
                        AgentRequest {
                            id: Default::default(),
                            session_id: session_id.clone(),
                            message: Message::user(format!(
                                r#"
**Execute task immediately**: task_id: {}
- **CurrentTime**: {}
- **Task Detail**:
```markdown
{}
```
- **Tips**: If the task fails, you are authorized to retry or ignore it at your discretion.
                                            "#,
                                &task.id,
                                now.to_rfc3339(),
                                task.full_desc()
                            )),
                        },
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            "Task '{}' (id: {}) is ready to execute based on cron schedule: {}",
                            task.name, task.id, &task.task_schedule
                        );
                    }
                    Err(err) => {
                        error!(
                            "Failed to send agent request for task '{}': {}",
                            task.name, err
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
