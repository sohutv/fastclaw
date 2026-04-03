use crate::agent::{Agent, AgentRequest};
use crate::channels::{Anonymous, SessionId};
use crate::config::{Config, Workspace};
use crate::tools::TaskTools;
use log::{error, info};
use rig::completion::Message;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

impl super::Heartbeat {
    pub(super) async fn spawn_cron_tasks<F, R>(
        agent: Arc<dyn Agent>,
        _: &'static Config,
        workspace: &Workspace,
        _: Duration,
        task_submitter: F,
    ) -> crate::Result<()>
    where
        R: Future<Output = crate::Result<()>> + Send + Sync,
        F: (Fn(Arc<dyn Agent>, AgentRequest) -> R),
    {
        let tasks = TaskTools::fetch_ready_tasks(workspace).await?;
        let now = chrono::Local::now();
        for task in tasks {
            let cron = task.cron;
            // Parse cron expression and check if current time matches
            match cron::Schedule::from_str(&cron) {
                Ok(schedule) => {
                    let last_exe_at = task.last_exe_at.unwrap_or(task.created_at);
                    if let Some(next) = schedule.after(&last_exe_at).next() {
                        if next < now {
                            let session_id = SessionId::Anonymous {
                                val: Anonymous(task.session_id.clone()),
                                settings: Default::default(),
                            };
                            match task_submitter(
                                Arc::clone(&agent),
                                AgentRequest {
                                    id: Default::default(),
                                    session_id: session_id,
                                    message: Message::user(format!(
                                        r#"
**Execute task immediately**: task_id: {}
- **CurrentTime**: {}
- **Tips**: If the task fails, you are authorized to retry or ignore it at your discretion.
                                            "#,
                                        task.id,
                                        now.to_rfc3339(),
                                    )),
                                },
                            )
                            .await
                            {
                                Ok(_) => {
                                    info!(
                                        "Task '{}' (id: {}) is ready to execute based on cron schedule: {}, next trigger: {}",
                                        task.name, task.id, cron, next
                                    );
                                    if let Err(err) =
                                        TaskTools::mark_task_executed(workspace, task.id).await
                                    {
                                        error!(
                                            "Failed to update last_exe_at for task '{}' (id: {}): {}",
                                            task.name, task.id, err
                                        );
                                    }
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
                }
                Err(e) => {
                    error!(
                        "Failed to parse cron expression '{}' for task {}: {}",
                        cron, task.id, e
                    );
                }
            }
        }
        Ok(())
    }
}
