use crate::agent::{Agent, AgentRequest, LlmAgentSupplier, Workspace};
use crate::channels::SessionId;
use crate::config::Config;
use crate::model_provider::ModelProviders;
use rig::completion::Message;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

pub struct Heartbeat {
    config: &'static Config,
    agent: Box<dyn Agent>,
}

impl Heartbeat {
    pub async fn new(
        config: &'static Config,
        workspace: &'static Workspace,
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
        Ok(Self { config, agent })
    }

    pub async fn start(
        &mut self,
        agent_message_sender: Sender<AgentRequest>,
    ) -> crate::Result<JoinHandle<()>> {
        let mut interval =
            tokio::time::interval(Duration::from_secs(self.config.heartbeat_config.interval));
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let _ = agent_message_sender.send(AgentRequest{
                            session_id:SessionId::from(""),
                            message:Message::user(format!(r#"
You are a heartbeat scheduler. Review the following periodic tasks and decide whether any should be executed right now.
Check you task list in cron/TASKS.md
**CurrentTime**:{},
Consider:
- Task priority (high tasks are more urgent)
- Whether the task is time-sensitive or can wait
- Whether running the task now would provide value
             "#, chrono::Local::now().to_rfc3339())).into(),
                        }).await;
                    },
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
        });
        Ok(handle)
    }
}
