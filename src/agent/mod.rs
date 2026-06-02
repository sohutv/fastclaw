use crate::channels::{ChannelMessage, SessionId};
use anyhow::anyhow;
use async_trait::async_trait;
use derive_more::{Deref, Display, From, FromStr, Into};
use rig::completion::Usage;
use rig::message::{Message, Reasoning, ToolCall};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
use crate::memory::MemoryManager;
use crate::model_provider::{ModelProviderName, ModelProviders, ModelSettings, ReasoningEffort};

mod tool_filter {
    use async_trait::async_trait;
    use derive_more::Deref;
    use rig::tool::ToolDyn;
    use std::sync::Arc;

    pub trait Filter {
        fn filter(&self, tool: Box<dyn ToolDyn>) -> Option<Box<dyn ToolDyn>>;
    }

    #[async_trait]
    impl<F> Filter for F
    where
        F: Fn(Box<dyn ToolDyn>) -> Option<Box<dyn ToolDyn>> + Sync + Send,
    {
        fn filter(&self, tool: Box<dyn ToolDyn>) -> Option<Box<dyn ToolDyn>> {
            self(tool)
        }
    }

    #[derive(Clone, Deref)]
    pub struct ToolFilter(Arc<dyn Filter + Send + Sync>);

    impl Default for ToolFilter {
        fn default() -> Self {
            Self::from(|tool| Some(tool))
        }
    }

    impl<F> From<F> for ToolFilter
    where
        F: Filter + Send + Sync + 'static,
    {
        fn from(value: F) -> Self {
            Self(Arc::new(value))
        }
    }

    impl AsRef<Arc<dyn Filter + Sync + Send + 'static>> for ToolFilter {
        fn as_ref(&self) -> &Arc<dyn Filter + Sync + Send + 'static> {
            &self.0
        }
    }
}

use crate::tools::mcp_tool::McpRegistry;
use crate::type_::SystemPrompt;
pub use tool_filter::ToolFilter;

#[async_trait]
pub trait SessionCompactSupport: Send + Sync {
    async fn session_compact(
        self: Arc<Self>,
        channel_message_sender: Sender<crate::Result<ChannelMessage>>,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> HistoryCompactResult;
}
#[async_trait]
pub trait Agent: SessionCompactSupport + AgentClone + Send + Sync {
    async fn start(self: Arc<Self>) -> crate::Result<Arc<dyn Agent>>;

    async fn get_channel_sender(
        &self,
    ) -> crate::Result<Sender<(AgentRequest, AgentRequestContext)>>;

    fn context(&self) -> &AgentContext;

    #[allow(unused)]
    fn model_settings(&self) -> &ModelSettings;

    async fn fork_child(
        &self,
        id: AgentId,
        model_provider: &ModelProviderName,
        model_name: &ModelName,
        system_prompt: Option<SystemPrompt>,
    ) -> crate::Result<Arc<dyn Agent>> {
        let context = self.agent_context();
        let mut children = context.children.write().await;
        if let Some(agent) = children.get(&id) {
            Ok(Arc::clone(agent))
        } else {
            let agent = match context.config.model_provider(model_provider)? {
                ModelProviders::OpenaiCompatible(model_provider) => {
                    model_provider
                        .create_agent(
                            id,
                            context.config,
                            model_name.clone(),
                            Arc::clone(&context.history_manager),
                            Arc::clone(&context.memory_manager),
                            context.workspace,
                            system_prompt,
                            context.mcp_registry,
                        )
                        .await?
                }
            };
            let agent = (Arc::new(agent) as Arc<dyn Agent>).start().await?;
            children.insert(agent.id().clone(), Arc::clone(&agent));
            Ok(agent)
        }
    }

    async fn drop_child(&self, id: &AgentId) -> crate::Result<Arc<dyn Agent>> {
        let context = self.agent_context();
        let child = {
            let mut children = context.children.write().await;
            children.remove(id)
        }
        .ok_or(anyhow!("child {} agent not exist", id))?;
        Ok(child)
    }

    fn id(&self) -> &AgentId;
    fn agent_context(&self) -> Arc<AgentContext>;
}

#[async_trait]
pub trait AgentClone: Send + Sync {
    async fn clone_with(&self, id: AgentId) -> crate::Result<Arc<dyn Agent>>;
}

#[allow(unused)]
#[derive(Clone)]
pub struct AgentContext {
    pub config: &'static Config,
    pub workspace: &'static Workspace,
    pub history_manager: Arc<dyn HistoryManager>,
    pub memory_manager: Arc<MemoryManager>,
    pub children: Arc<RwLock<HashMap<AgentId, Arc<dyn Agent>>>>,
    pub system_prompt: Option<SystemPrompt>,
    pub mcp_registry: &'static McpRegistry,
}

#[derive(
    Debug, Clone, Deref, Eq, PartialEq, Ord, PartialOrd, Display, Serialize, Deserialize, Hash,
)]
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
        history_manager: Arc<dyn HistoryManager>,
        memory_manager: Arc<MemoryManager>,
        workspace: &'static Workspace,
        system_prompt: Option<SystemPrompt>,
        mcp_registry: &'static McpRegistry,
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

#[derive(Clone)]
pub struct AgentRequestContext {
    pub channel_message_sender: Sender<crate::Result<ChannelMessage>>,
    pub addi_system_prompt: Option<String>,
    pub tool_filter: ToolFilter,
    pub with_history: bool,
}

#[derive(Debug, Clone, Deref, Display, From, FromStr)]
pub struct RequestId(String);

impl From<uuid::Uuid> for RequestId {
    fn from(value: uuid::Uuid) -> Self {
        value.to_string().into()
    }
}

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
