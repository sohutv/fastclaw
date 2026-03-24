use crate::channels::{ChannelMessage, SessionId};
use async_trait::async_trait;
use derive_more::{Deref, Display, Into};
use rig::completion::Usage;
use rig::message::{Message, Reasoning, ToolCall};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

mod jsonl_history_manager;
pub use jsonl_history_manager::JsonlHistoryManager;

mod llm_agent;
mod prompt;

use crate::config::Config;
use crate::model_provider::ModelName;

pub trait Agent: Send + Sync {
    fn run(
        self: Self,
        channel_message_sender: Sender<ChannelMessage>,
    ) -> crate::Result<(JoinHandle<()>, Sender<AgentRequest>)>;
}

#[async_trait]
pub trait HistoryManager: Send + Sync {
    async fn store(
        &mut self,
        session_id: &SessionId,
        agent: &AgentName,
        usage: &Usage,
        message: &[Message],
    ) -> crate::Result<()>;

    async fn load(&self, session_id: &SessionId, agent: &AgentName) -> crate::Result<Vec<Message>>;

    #[allow(unused)]
    async fn usage(&self, session_id: &SessionId, agent: &AgentName) -> crate::Result<Usage>;
}

#[allow(unused)]
#[derive(Clone)]
pub struct AgentContext {
    pub config: &'static Config,
    pub workspace: &'static Workspace,
    pub history_manager: Arc<RwLock<dyn HistoryManager>>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: PathBuf,
}

impl<P: AsRef<Path>> From<P> for Workspace {
    fn from(value: P) -> Self {
        Self {
            path: value.as_ref().join("workspace"),
        }
    }
}

#[derive(Debug, Clone, Deref, Eq, PartialEq, Ord, PartialOrd, Display)]
pub struct AgentName(String);

impl<S: Into<String>> From<S> for AgentName {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

#[async_trait]
pub trait LlmAgentSupplier {
    type A: Agent;
    async fn create_agent<N: Into<AgentName> + Send>(
        &self,
        name: N,
        config: &'static Config,
        model: ModelName,
        history_manager: &Arc<RwLock<dyn HistoryManager>>,
        workspace: &'static Workspace,
    ) -> crate::Result<Self::A>;
}

#[derive(Debug, Clone, Deref, Into)]
pub struct AgentRequest {
    pub session_id: SessionId,
    #[deref]
    #[into]
    pub message: Message,
}

#[derive(Clone)]
pub enum AgentResponse {
    Start,
    ToolCall(ToolCall),
    ReasoningStream(Reasoning),
    MessageStream(Message),
    Final(Usage),
    Error(String),
}
