use crate::agent::{Agent, AgentResponse};
use crate::config::Config;
use async_trait::async_trait;
use derive_more::{Deref, Display};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Deref;
use std::thread::JoinHandle;

#[cfg(feature = "channel_cli_channel")]
pub mod cli_channel;
mod console_cmd;
#[cfg(feature = "channel_dingtalk_channel")]
pub mod dingtalk_channel;

pub mod a2a_channel;

#[async_trait]
pub trait Channel {
    async fn start(
        self,
        agent: Box<dyn Agent>,
    ) -> crate::Result<JoinHandle<()>>;
}

#[allow(unused)]
#[derive(Clone)]
pub struct ChannelContext {
    pub config: Config,
    pub sessions: HashMap<SessionId, Session>,
}

#[derive(Clone, Deref)]
pub struct ChannelMessage {
    pub session_id: SessionId,
    #[deref]
    pub message: AgentResponse,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub enum Session {
    Private { session_id: SessionId },
    Group { session_id: SessionId },
}

#[derive(Debug, Clone, Deref, Eq, PartialEq, Hash, Serialize, Deserialize, Default, Display)]
pub struct SessionId(String);

impl<S: Into<String>> From<S> for SessionId {
    fn from(value: S) -> Self {
        SessionId(value.into())
    }
}

impl Deref for Session {
    type Target = SessionId;

    fn deref(&self) -> &Self::Target {
        match self {
            Session::Private { session_id } => session_id,
            Session::Group { session_id } => session_id,
        }
    }
}
