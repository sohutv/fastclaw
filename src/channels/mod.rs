use crate::agent::{Agent, AgentResponse, Workspace};
use crate::config::Config;
use async_trait::async_trait;
use derive_more::{Deref, Display};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::RwLock;

#[cfg(feature = "channel_cli_channel")]
pub mod cli_channel;
mod console_cmd;
#[cfg(feature = "channel_dingtalk_channel")]
pub mod dingtalk_channel;

pub mod a2a_channel;

#[async_trait]
pub trait Channel {
    async fn start(self, agent: Arc<dyn Agent>) -> crate::Result<JoinHandle<()>>;
}

#[allow(unused)]
#[derive(Clone)]
pub struct ChannelContext {
    pub config: Config,
    pub workspace: &'static Workspace,
    pub sessions: Arc<RwLock<HashMap<SessionId, Session>>>,
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
    Private {
        session_id: SessionId,
    },
    Group {
        session_id: SessionId,
        group_name: Option<String>,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Display)]
pub enum SessionId {
    Master(MasterSessionId),
    User(UserSessionId),
    Group(GroupSessionId),
}

impl SessionId {
    pub fn as_user_id(&self) -> &SessionId {
        match self {
            SessionId::Master(_) | SessionId::User(_) => self,
            SessionId::Group(GroupSessionId { user_id, .. }) => user_id,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Display, Default, Deref)]
pub struct MasterSessionId(String);

impl<S: Into<String>> From<S> for MasterSessionId {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Display, Default, Deref)]
pub struct UserSessionId(String);

impl<S: Into<String>> From<S> for UserSessionId {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Display, Default, Deref)]
#[display("{group_name:?}[{group_id}]:{user_id}")]
pub struct GroupSessionId {
    #[deref]
    group_id: Box<SessionId>,
    user_id: Box<SessionId>,
    group_name: Option<String>,
}

impl Eq for GroupSessionId {}

impl PartialEq for GroupSessionId {
    fn eq(&self, other: &Self) -> bool {
        self.group_id.eq(&other.group_id)
    }
}

impl Hash for GroupSessionId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.group_id.hash(state);
    }
}

impl Deref for SessionId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match self {
            SessionId::Master(val) => val,
            SessionId::User(val) => val,
            SessionId::Group(GroupSessionId { group_id, .. }) => group_id,
        }
    }
}
impl Default for SessionId {
    fn default() -> Self {
        SessionId::User(Default::default())
    }
}

impl<S: Into<String>> From<S> for SessionId {
    fn from(value: S) -> Self {
        SessionId::User(UserSessionId::from(value))
    }
}

impl Deref for Session {
    type Target = SessionId;

    fn deref(&self) -> &Self::Target {
        match self {
            Session::Private { session_id } => session_id,
            Session::Group { session_id, .. } => session_id,
        }
    }
}
