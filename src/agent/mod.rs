use crate::channels::{ChannelMessage, SessionId};
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::Local;
use derive_more::{Deref, Display, From, FromStr, Into};
use rig::OneOrMany;
use rig::completion::Usage;
use rig::message::{Message, Reasoning, ToolCall, UserContent};
use rig::providers::openai::responses_api::ReasoningEffort;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;

mod llm_agent;
mod prompt;
mod session_history;
use crate::ModelName;
use crate::config::{Config, Workspace};
use crate::memory::MemoryManager;
use crate::model_provider::{ModelProviderName, ModelProviders, ModelSettings};
pub use session_history::{HistoryManager, JsonlHistoryManager};

use crate::tools::mcp_tool::McpRegistry;
pub use crate::tools::tool_filter::ToolFilter;
use crate::tools::tool_filter::ToolNameFilter;
use crate::type_::SystemPrompt;

#[async_trait]
pub trait SessionCompactSupport: Send + Sync {
    async fn session_compact(
        self: Arc<Self>,
        channel_message_sender: Sender<crate::Result<ChannelMessage>>,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> HistoryCompactResult;
}

pub struct AgentRequestPkg {
    req: AgentRequest,
    ctx: AgentRequestContext,
    ack_sender: Option<tokio::sync::oneshot::Sender<()>>,
    #[allow(unused)]
    create_time: chrono::DateTime<Local>,
}

impl AgentRequestPkg {
    pub fn new(
        req: AgentRequest,
        ctx: AgentRequestContext,
        ack_sender: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> Self {
        Self {
            req,
            ctx,
            ack_sender,
            create_time: Local::now(),
        }
    }

    pub fn new_with_ack(
        req: AgentRequest,
        ctx: AgentRequestContext,
    ) -> (Self, tokio::sync::oneshot::Receiver<()>) {
        let (ack_sender, ack) = tokio::sync::oneshot::channel();
        (Self::new(req, ctx, Some(ack_sender)), ack)
    }

    pub fn new_without_ack(req: AgentRequest, ctx: AgentRequestContext) -> Self {
        Self::new(req, ctx, None)
    }
}

#[async_trait]
pub trait Agent: SessionCompactSupport + AgentClone + Send + Sync {
    async fn start(self: Arc<Self>) -> crate::Result<Arc<dyn Agent>>;

    async fn get_channel_sender(&self) -> crate::Result<Sender<AgentRequestPkg>>;

    fn context(&self) -> &AgentContext;

    #[allow(unused)]
    fn model_settings(&self) -> &ModelSettings;

    fn agent_settings(&self) -> &AgentSettings;

    async fn fork_child(
        &self,
        id: &AgentId,
        group: &AgentGroup,
        model_provider: &ModelProviderName,
        model_name: &ModelName,
        system_prompt: Option<SystemPrompt>,
        agent_settings: AgentSettings,
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
                            group,
                            context.config,
                            model_name.clone(),
                            Arc::clone(&context.history_manager),
                            Arc::clone(&context.memory_manager),
                            context.workspace,
                            system_prompt,
                            context.mcp_registry,
                            agent_settings,
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

    #[allow(unused)]
    fn agent_group(&self) -> &AgentGroup;
}

#[async_trait]
pub trait AgentClone: Send + Sync {
    async fn clone_with(
        &self,
        id: AgentId,
        agent_settings: Option<AgentSettings>,
    ) -> crate::Result<Arc<dyn Agent>>;
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
#[serde(default)]
pub struct AgentId(String);

impl Default for AgentId {
    fn default() -> Self {
        Self("main".to_string())
    }
}

impl<S: Into<String>> From<S> for AgentId {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

#[async_trait]
pub trait LlmAgentSupplier {
    type A: Agent;
    async fn create_agent(
        &self,
        agent_id: &AgentId,
        group: &AgentGroup,
        config: &'static Config,
        model: ModelName,
        history_manager: Arc<dyn HistoryManager>,
        memory_manager: Arc<MemoryManager>,
        workspace: &'static Workspace,
        system_prompt: Option<SystemPrompt>,
        mcp_registry: &'static McpRegistry,
        agent_settings: AgentSettings,
    ) -> crate::Result<Self::A>;
}

#[derive(Debug, Clone, Deref, Into)]
pub struct AgentRequest {
    pub id: RequestId,
    pub session_id: SessionId,
    #[deref]
    #[into]
    pub message: Vec<OneOrMany<UserContent>>,
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

#[derive(
    Debug, Clone, Deref, Eq, PartialEq, Ord, PartialOrd, Display, Serialize, Deserialize, Hash,
)]
pub struct AgentGroup(String);
impl From<AgentId> for AgentGroup {
    fn from(value: AgentId) -> Self {
        Self(value.0)
    }
}
impl<S: Into<String>> From<S> for AgentGroup {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub max_tokens: Option<u64>,
    pub temperature: f64,
    pub max_turns: usize,
    pub reasoning_effort: ReasoningEffort,
    pub compact_threshold: f32,
    pub task_queue_size: TaskQueueSize,
    pub task_backpressure: TaskBackpressure,
    pub task_agg_window: TaskAggWindow,
    pub chat_history_limit: Option<usize>,
    pub history_compact_enable: bool,
    pub tool_filter: Option<ToolNameFilter>,
    pub output_schema: Option<schemars::Schema>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Deref)]
pub struct TaskQueueSize(usize);
impl Default for TaskQueueSize {
    fn default() -> Self {
        Self(8)
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum TaskBackpressure {
    #[default]
    #[serde(alias = "pending")]
    Pending,
    #[serde(alias = "latest", alias = "drop")]
    Latest,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TaskAggWindow {
    SlidingWindow(usize),
    TumblingWindow(usize),
}

impl Default for TaskAggWindow {
    fn default() -> Self {
        Self::TumblingWindow(1)
    }
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_tokens: None,
            temperature: 1.,
            max_turns: 256,
            compact_threshold: 0.8,
            reasoning_effort: Default::default(),
            task_queue_size: Default::default(),
            task_backpressure: Default::default(),
            task_agg_window: Default::default(),
            chat_history_limit: None,
            history_compact_enable: true,
            tool_filter: None,
            output_schema: None,
        }
    }
}
