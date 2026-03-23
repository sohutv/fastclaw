use crate::agent::{AgentMessage, AgentMessageSender};
use crate::channels::SessionId;
use crate::config::Config;
use rig::completion::Message;
use std::time::Duration;
use tokio::task::JoinHandle;

pub struct Heartbeat {
    config: &'static Config,
}

impl Heartbeat {
    pub fn new(config: &'static Config) -> crate::Result<Self> {
        Ok(Self { config })
    }

    pub async fn start(
        &mut self,
        agent_message_sender: AgentMessageSender,
    ) -> crate::Result<JoinHandle<()>> {
        let mut interval =
            tokio::time::interval(Duration::from_secs(self.config.heartbeat_config.interval));
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let _ = agent_message_sender.send(AgentMessage::Private {
                            session_id: SessionId::from("heartbeat".to_string()),
                            message: Message::user(format!(r#"
You are a heartbeat scheduler. Review the following periodic tasks and decide whether any should be executed right now.
Check you task list in cron/TASKS.md
**CurrentTime**:{},
Consider:
- Task priority (high tasks are more urgent)
- Whether the task is time-sensitive or can wait
- Whether running the task now would provide value
             "#, chrono::Local::now().to_rfc3339()))
                        }).await;
                    },
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
        });
        Ok(handle)
    }
}
