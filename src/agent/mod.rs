use crate::channels::{ChannelMessage, SessionId};
use async_trait::async_trait;
use derive_more::{Deref, Display, Into};
use rig::completion::Usage;
use rig::message::{Message, Reasoning, ToolCall};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;

mod llm_agent;
mod prompt;

mod session_history;
pub use session_history::{HistoryManager, JsonlHistoryManager};

use crate::config::Config;
use crate::model_provider::ModelName;

#[async_trait]
pub trait Agent: Send + Sync {
    async fn run(
        &self,
        request: AgentRequest,
        channel_message_sender: Sender<ChannelMessage>,
    ) -> crate::Result<()>;

    async fn session_compact(
        &self,
        channel_message_sender: Sender<ChannelMessage>,
        session_id: &SessionId,
    ) -> HistoryCompactResult;
}

#[allow(unused)]
#[derive(Clone)]
pub struct AgentContext {
    pub config: &'static Config,
    pub workspace: &'static Workspace,
    pub history_manager: Option<Arc<RwLock<dyn HistoryManager>>>,
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

#[derive(Debug, Clone, Deref, Eq, PartialEq, Ord, PartialOrd, Display, Serialize, Deserialize)]
pub struct AgentId(String);

impl<S: Into<String>> From<S> for AgentId {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

#[async_trait]
pub trait LlmAgentSupplier {
    type A: Agent;
    async fn create_agent<N: Into<AgentId> + Send>(
        &self,
        name: N,
        config: &'static Config,
        model: ModelName,
        history_manager: Option<Arc<RwLock<dyn HistoryManager>>>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentResponse {
    Start,
    ToolCall(ToolCall),
    ReasoningStream(Reasoning),
    MessageStream(Message),
    Final(Usage),
    Error(String),
    Notify(Notify),
    HistoryCompact(HistoryCompactResult),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notify {
    pub title: String,
    pub content: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HistoryCompactResult {
    Ok(HistoryCompactVal),
    Err(String),
    Ignore(String),
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HistoryCompactVal {
    current: Usage,
    before: Usage,
    compact_ratio: f64,
}

impl HistoryCompactVal {
    pub fn new(before: Usage, after: Usage) -> Self {
        Self {
            current: Usage {
                total_tokens: after.output_tokens,
                ..after
            },
            before,
            compact_ratio: (1. - (after.output_tokens as f64 / before.total_tokens as f64)) * 100.,
        }
    }

    pub fn current(&self) -> &Usage {
        &self.current
    }

    pub fn before(&self) -> &Usage {
        &self.before
    }

    pub fn compact_ratio(&self) -> f64 {
        self.compact_ratio
    }
}

impl Display for HistoryCompactVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "total usage {} -> {}, compression ratio: {:.2}%",
            self.before.total_tokens, self.current.total_tokens, self.compact_ratio
        )
    }
}
