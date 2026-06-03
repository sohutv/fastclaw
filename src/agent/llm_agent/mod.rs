use crate::ModelName;
use crate::agent::{
    Agent, AgentClone, AgentContext, AgentId, AgentRequest, AgentRequestContext, AgentSettings,
    HistoryManager, LlmAgentSupplier, Workspace,
};
use crate::config::Config;
use crate::memory::MemoryManager;
use crate::model_provider::{ModelProvider, ModelSettings};
use crate::tools::mcp_tool::McpRegistry;
use crate::type_::SystemPrompt;
use anyhow::anyhow;
use async_trait::async_trait;
use rig::client::CompletionClient;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

mod create_agent;
mod handle_history;
mod handle_request;
mod session_compact;

#[derive(Clone)]
pub struct LlmAgent<C, P>
where
    C: CompletionClient,
    P: ModelProvider<Client = C>,
{
    id: AgentId,
    ctx: Arc<AgentContext>,
    model_provider: P,
    model_name: ModelName,
    pub model_settings: ModelSettings,
    agent_settings: AgentSettings,
    channel_sender: Arc<tokio::sync::RwLock<Option<Sender<(AgentRequest, AgentRequestContext)>>>>,
}

#[async_trait]
impl<C, P> LlmAgentSupplier for P
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    type A = LlmAgent<C, P>;

    async fn create_agent<ID: Into<AgentId> + Send>(
        &self,
        agent_id: ID,
        config: &'static Config,
        model: ModelName,
        history_manager: Arc<dyn HistoryManager>,
        memory_manager: Arc<MemoryManager>,
        workspace: &'static Workspace,
        system_prompt: Option<SystemPrompt>,
        mcp_registry: &'static McpRegistry,
    ) -> crate::Result<Self::A> {
        Ok(LlmAgent::new(
            agent_id.into(),
            config,
            self.clone(),
            model,
            history_manager,
            memory_manager,
            workspace,
            system_prompt,
            mcp_registry,
        )
        .await?)
    }
}

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    async fn new(
        agent_id: AgentId,
        config: &'static Config,
        model_provider: P,
        model_name: ModelName,
        history_manager: Arc<dyn HistoryManager>,
        memory_manager: Arc<MemoryManager>,
        workspace: &'static Workspace,
        system_prompt: Option<SystemPrompt>,
        mcp_registry: &'static McpRegistry,
    ) -> crate::Result<Self> {
        let ctx = Arc::new(AgentContext {
            config,
            workspace,
            history_manager,
            memory_manager,
            children: Default::default(),
            system_prompt,
            mcp_registry,
        });
        Ok(Self {
            model_settings: model_provider
                .model_settings(&model_name)
                .map(|it| it.clone())
                .ok_or(anyhow!("model settings not found for {}", agent_id))?,
            agent_settings: ctx
                .config
                .agent_settings(&agent_id)
                .map(|it| it.clone())
                .unwrap_or_default(),
            model_name,
            model_provider,
            id: agent_id,
            ctx,
            channel_sender: Default::default(),
        })
    }
}

#[async_trait]
impl<C, P> AgentClone for LlmAgent<C, P>
where
    C: 'static + CompletionClient + Send + Sync,
    P: 'static + ModelProvider<Client = C> + Send + Sync,
{
    async fn clone_with(&self, id: AgentId) -> crate::Result<Arc<dyn Agent>> {
        if id.eq(&self.id) {
            return Err(anyhow!("clone agent failed, duplicated id: {id}"));
        }
        let agent = Self {
            id,
            model_settings: self.model_settings.clone(),
            agent_settings: self.agent_settings.clone(),
            model_name: self.model_name.clone(),
            model_provider: self.model_provider.clone(),
            ctx: self.ctx.clone(),
            channel_sender: Default::default(),
        };
        Ok(Arc::new(agent))
    }
}

#[async_trait]
impl<C, P> Agent for LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    async fn start(self: Arc<Self>) -> crate::Result<Arc<dyn Agent>> {
        {
            let mut sender = self.channel_sender.write().await;
            if sender.is_some() {
                drop(sender);
                return Ok(self);
            }
            let (tx, mut rx) = tokio::sync::mpsc::channel(*self.agent_settings.task_queue_size);
            let self_ = Arc::clone(&self);
            tokio::spawn(async move {
                while let Some((request, ctx)) = rx.recv().await {
                    Arc::clone(&self_).handle_request(request, ctx).await
                }
            });
            *sender = Some(tx);
        }
        Ok(self)
    }

    async fn get_channel_sender(
        &self,
    ) -> crate::Result<Sender<(AgentRequest, AgentRequestContext)>> {
        let sender = self.channel_sender.read().await;
        if let Some(sender) = &*sender {
            Ok(sender.clone())
        } else {
            Err(anyhow!(
                "Agent channel not initialized. Call start() first."
            ))
        }
    }

    fn context(&self) -> &AgentContext {
        &self.ctx
    }

    fn model_settings(&self) -> &ModelSettings {
        &self.model_settings
    }

    fn agent_settings(&self) -> &AgentSettings {
        &self.agent_settings
    }

    fn id(&self) -> &AgentId {
        &self.id
    }
    fn agent_context(&self) -> Arc<AgentContext> {
        Arc::clone(&self.ctx)
    }
}
