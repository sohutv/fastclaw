use crate::channels::{ChannelMessage, SessionId};
use async_trait::async_trait;
use derive_more::{Deref, Display, Into};
use rig::completion::Usage;
use rig::message::{Message, Reasoning, ToolCall};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;

mod llm_agent;
mod prompt;

mod session_history;
pub use session_history::{HistoryManager, JsonlHistoryManager};

use crate::ModelName;
use crate::config::{Config, Workspace};
use crate::model_provider::{ModelProviderName, ReasoningEffort};
#[async_trait]
pub trait Agent: Send + Sync {
    async fn run(
        &self,
        request: AgentRequest,
        channel_message_sender: Sender<ChannelMessage>,
        addi_system_prompt: Option<&str>,
    ) -> crate::Result<()>;

    async fn session_compact(
        &self,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> HistoryCompactResult;
}

#[allow(unused)]
#[derive(Clone)]
pub struct AgentContext {
    pub config: &'static Config,
    pub workspace: &'static Workspace,
    pub history_manager: Option<Arc<RwLock<dyn HistoryManager>>>,
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
    pub id: RequestId,
    pub session_id: SessionId,
    #[deref]
    #[into]
    pub message: Message,
}

#[derive(Debug, Clone, Deref, Display)]
pub struct RequestId(String);
impl Default for RequestId {
    fn default() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
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
pub enum Notify {
    Text(String),
    Markdown { title: String, content: String },
}

impl<S: Into<String>> From<S> for Notify {
    fn from(value: S) -> Self {
        Notify::Text(value.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HistoryCompactResult {
    Ok(HistoryCompactVal),
    Err(String),
    Ignore(String),
}

impl<Err: std::fmt::Display> From<Err> for HistoryCompactResult {
    fn from(value: Err) -> Self {
        HistoryCompactResult::Err(value.to_string())
    }
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
            current: after,
            before,
            compact_ratio: (1. - (after.total_tokens as f64 / before.total_tokens as f64)) * 100.,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub model_provider: Option<ModelProviderName>,
    pub model: Option<ModelName>,
    pub show_reasoning: Option<bool>,
    pub max_tokens: Option<u64>,
    pub temperature: f64,
    pub max_turns: usize,
    pub reasoning_effort: ReasoningEffort,
    pub compact_threshold: f32,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            model_provider: None,
            model: None,
            show_reasoning: None,
            max_tokens: None,
            temperature: 1.,
            max_turns: 256,
            compact_threshold: 0.8,
            reasoning_effort: Default::default(),
        }
    }
}
