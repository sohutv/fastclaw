use crate::agent::{Agent, AgentRequest, HistoryManager, LlmAgentSupplier};
use crate::channels::SessionId;
use crate::config::{Config, Workspace};
use crate::model_provider::ModelProviders;
use log::error;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

mod spawn_cron_tasks;

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

    pub async fn start<F, R>(
        self,
        session_ids: Vec<SessionId>,
        agent: Arc<dyn Agent>,
        task_submitter: F,
    ) -> crate::Result<JoinHandle<()>>
    where
        R: Future<Output = crate::Result<()>> + Send,
        F: (Fn(Arc<dyn Agent>, AgentRequest) -> R) + Clone + Sync + Send + 'static,
    {
        let config = self.config;
        let workspace = self.workspace;
        let mut interval = tokio::time::interval(self.interval);

        let handle = {
            tokio::spawn(async move {
                let agent = Arc::clone(&agent);
                let task_submitter = task_submitter;
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            match Self::spawn_cron_tasks(Arc::clone(&agent), config,workspace, &session_ids,task_submitter.clone()).await{
                                Ok(_)=>{}
                                Err(e)=>{error!("Failed to fetch cron tasks: {}",e)}
                            }
                        },
                        _ = tokio::signal::ctrl_c() => break,
                    }
                }
            })
        };
        Ok(handle)
    }
}
