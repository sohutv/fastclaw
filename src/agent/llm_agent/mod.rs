use crate::ModelName;
use crate::agent::{
    Agent, AgentContext, AgentId, AgentRequest, AgentRequestContext, AgentSettings, HistoryManager,
    LlmAgentSupplier, Workspace,
};
use crate::config::Config;
use crate::memory::MemoryManager;
use crate::model_provider::{ModelProvider, ModelSettings};
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
    ) -> crate::Result<Self::A> {
        Ok(LlmAgent::new(
            agent_id.into(),
            config,
            self.clone(),
            model,
            history_manager,
            memory_manager,
            workspace,
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
    ) -> crate::Result<Self> {
        let ctx = Arc::new(AgentContext {
            config,
            workspace,
            history_manager,
            memory_manager,
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

    #[allow(unused)]
    pub async fn fork<ID: Into<AgentId>>(&self) -> crate::Result<Self> {
        self.fork_with(self.id.clone()).await
    }

    pub async fn fork_with<ID: Into<AgentId>>(&self, agent_id: ID) -> crate::Result<Self> {
        Ok(Self {
            id: agent_id.into(),
            model_settings: self.model_settings.clone(),
            agent_settings: self.agent_settings.clone(),
            model_name: self.model_name.clone(),
            model_provider: self.model_provider.clone(),
            ctx: self.ctx.clone(),
            channel_sender: Default::default(),
        })
    }
}

#[async_trait]
impl<C, P> Agent for LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    async fn start(self: Arc<Self>) -> crate::Result<()> {
        let mut sender = self.channel_sender.write().await;
        if sender.is_some() {
            return Ok(());
        }
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let self_ = Arc::clone(&self);
        tokio::spawn(async move {
            while let Some((request, ctx)) = rx.recv().await {
                self_.handle_request(request, ctx).await
            }
        });
        *sender = Some(tx);
        Ok(())
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
}
