use crate::agent::{Agent, AgentRequest, HistoryManager, LlmAgentSupplier};
use crate::channels::{Anonymous, SessionId};
use crate::config::{Config, Workspace};
use crate::model_provider::ModelProviders;
use crate::tools::TaskTools;
use log::{error, info};
use rig::completion::Message;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

#[allow(unused)]
pub struct Heartbeat {
    config: &'static Config,
    workspace: &'static Workspace,
    history_manager: Arc<RwLock<dyn HistoryManager>>,
    agent: Box<dyn Agent>,
    interval: Duration,
}

impl Heartbeat {
    pub async fn new(
        config: &'static Config,
        workspace: &'static Workspace,
        history_manager: &Arc<RwLock<dyn HistoryManager>>,
    ) -> crate::Result<Self> {
        let agent = Box::new(match config.default_model_provider()? {
            ModelProviders::OpenaiCompatible(model_provider) => {
                model_provider
                    .create_agent(
                        "heartbeat",
                        config,
                        config.default_model().clone(),
                        None,
                        workspace,
                    )
                    .await?
            }
        });
        Ok(Self {
            config,
            workspace,
            history_manager: Arc::clone(history_manager),
            agent,
            interval: Duration::from_secs(config.heartbeat_config.interval),
        })
    }

    pub async fn start(
        &mut self,
        agent_message_sender: Sender<AgentRequest>,
    ) -> crate::Result<JoinHandle<()>> {
        let config = self.config;
        let workspace = self.workspace;
        let interval_dur = self.interval;
        let mut interval = tokio::time::interval(interval_dur);
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        match Self::spawn_cron_tasks(config,workspace, interval_dur,agent_message_sender.clone()).await{
                            Ok(_)=>{}
                            Err(e)=>{error!("Failed to fetch cron tasks: {}",e)}
                        }
                    },
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
        });
        Ok(handle)
    }
}

impl Heartbeat {
    async fn spawn_cron_tasks(
        _: &'static Config,
        workspace: &Workspace,
        interval: Duration,
        agent_message_sender: Sender<AgentRequest>,
    ) -> crate::Result<()> {
        let tasks = TaskTools::fetch_ready_tasks(workspace).await?;
        let now = chrono::Local::now();
        for task in tasks {
            let cron = task.cron;
            // Parse cron expression and check if current time matches
            match cron::Schedule::from_str(&cron) {
                Ok(schedule) => {
                    // Check if task should execute within the next minute
                    if let Some(next) = schedule.upcoming(chrono::Local).next() {
                        let time_until_seconds = next.signed_duration_since(now).num_seconds();
                        if time_until_seconds < interval.as_secs() as i64 && time_until_seconds >= 0
                        {
                            let session_id = SessionId::Anonymous {
                                val: Anonymous(task.session_id.clone()),
                                settings: Default::default(),
                            };
                            let agent_message_sender = agent_message_sender.clone();
                            let _ = tokio::spawn(async move {
                                match agent_message_sender
                                    .send(AgentRequest {
                                        session_id: session_id,
                                        message: Message::user(format!(
                                            r#"
**Execute task immediately**: task_id: {}
**Tips**: If the task fails, you are authorized to retry or ignore it at your discretion.
                                            "#,
                                            task.id
                                        )),
                                    })
                                    .await
                                {
                                    Ok(_) => {
                                        info!(
                                            "Task '{}' (id: {}) is ready to execute based on cron schedule: {}",
                                            task.name, task.id, cron
                                        );
                                    }
                                    Err(err) => {
                                        error!(
                                            "Failed to send agent request for task '{}': {}",
                                            task.name, err
                                        );
                                    }
                                }
                            });
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
