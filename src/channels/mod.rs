use crate::agent::{Agent, AgentRequest, AgentResponse};
use crate::config::{Config, Workspace};
use async_trait::async_trait;
use derive_more::Deref;
use log::{error, info};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

#[cfg(feature = "channel_cli_channel")]
pub mod cli_channel;
mod console_cmd;
#[cfg(feature = "channel_dingtalk_channel")]
pub mod dingtalk_channel;

#[cfg(feature = "channel_wechat_channel")]
pub mod wechat_channel;

pub mod a2a_channel;
mod session_id;
pub use session_id::*;
#[async_trait]
pub trait Channel {
    type Output;

    async fn start(self, agent: Arc<dyn Agent>) -> crate::Result<Self::Output>;
}

async fn spawn_agent_task<F>(
    req: AgentRequest,
    agent_supplier: F,
    addi_system_prompt: Option<String>,
) -> crate::Result<Receiver<ChannelMessage>>
where
    F: FnOnce() -> Arc<dyn Agent>,
{
    let agent = agent_supplier();
    let (channel_message_sender, channel_message_receiver) = tokio::sync::mpsc::channel(32);
    tokio::spawn(async move {
        let task_id = req.id.clone();
        match agent
            .run(
                req,
                channel_message_sender.clone(),
                addi_system_prompt.as_deref(),
            )
            .await
        {
            Ok(_) => {
                info!("Agent run completed, task_id: {}", task_id);
            }
            Err(err) => {
                error!("Agent run failed, task_id: {}, error: {}", task_id, err);
            }
        }
    });
    Ok(channel_message_receiver)
}

#[allow(unused)]
#[derive(Clone)]
pub struct ChannelContext {
    pub config: Config,
    pub workspace: &'static Workspace,
}

#[derive(Clone, Deref)]
pub struct ChannelMessage {
    pub session_id: SessionId,
    #[deref]
    pub message: AgentResponse,
}
