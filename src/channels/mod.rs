use crate::agent::{Agent, AgentRequest, AgentResponse};
use crate::config::{Config, Workspace};
use async_trait::async_trait;
use derive_more::Deref;
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc::Sender;

#[cfg(feature = "channel_cli_channel")]
pub mod cli_channel;
mod console_cmd;
#[cfg(feature = "channel_dingtalk_channel")]
pub mod dingtalk_channel;

pub mod a2a_channel;
mod session_id;
pub use session_id::*;
#[async_trait]
pub trait Channel {
    async fn start(
        self,
        agent: Arc<dyn Agent>,
    ) -> crate::Result<(Sender<AgentRequest>, JoinHandle<()>)>;
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
