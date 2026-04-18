use crate::agent::{Agent, HistoryManager};
use crate::channels::Channel;
use crate::config::{Config, Workspace};
use log::error;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

mod spawn_cron_tasks;

#[allow(unused)]
pub struct Heartbeat<C, Client> {
    config: &'static Config,
    workspace: &'static Workspace,
    history_manager: Arc<RwLock<dyn HistoryManager>>,
    interval: Duration,
    channel: Arc<C>,
    client: Arc<Client>,
    agent: Arc<dyn Agent>,
}

impl<C, Client> Heartbeat<C, Client>
where
    C: Channel<Client = Client>,
{
    pub fn new(
        config: &'static Config,
        workspace: &'static Workspace,
        history_manager: &Arc<RwLock<dyn HistoryManager>>,
        channel: Arc<C>,
        client: Arc<Client>,
        agent: Arc<dyn Agent>,
    ) -> crate::Result<Self> {
        Ok(Self {
            config,
            workspace,
            history_manager: Arc::clone(history_manager),
            channel,
            client,
            agent,
            interval: Duration::from_secs(config.heartbeat_config.interval),
        })
    }

    pub async fn start(self) -> crate::Result<(Arc<Self>, JoinHandle<()>)>
    where
        Client: Send + Sync + 'static,
        C: Channel<Client = Client>,
    {
        let self_ = Arc::new(self);
        let handle = {
            let self_ = Arc::clone(&self_);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(self_.interval);
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            match self_.spawn_cron_tasks().await{
                                Ok(_)=>{}
                                Err(e)=>{error!("Failed to fetch cron tasks: {}",e)}
                            }
                        },
                        _ = tokio::signal::ctrl_c() => break,
                    }
                }
            })
        };
        Ok((self_, handle))
    }
}
