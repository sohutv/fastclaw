use crate::channels::{ChannelMessage, SessionId};
use anyhow::anyhow;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

mod llm_agent;
mod prompt;
mod session_history;
use crate::ModelName;
use crate::config::Workspace;
use crate::model_provider::ModelSettings;
pub use session_history::{HistoryManager, JsonlHistoryManager};

pub use crate::tools::tool_filter::ToolFilter;
use crate::type_::SystemPrompt;

#[async_trait]
pub trait LlmAgentSupplier {
    type A: Agent;
    async fn create_agent(
        &self,
        agent_id: &AgentId,
        group: &AgentGroup,
        model: ModelName,
        agent_settings: AgentSettings,
        description: Option<String>,
        owner_session: &OwnerSession,
        system_prompt: Arc<dyn SystemPromptProvider>,
        agent_context: &'static AgentContext,
    ) -> crate::Result<Self::A>;
}

#[async_trait]
pub trait AgentVisitor {
    fn id(&self) -> &AgentId;
    fn context(&self) -> &'static AgentContext;

    fn agent_group(&self) -> &AgentGroup;

    fn description(&self) -> &str;

    #[allow(unused)]
    fn owner_session(&self) -> &OwnerSession;

    #[allow(unused)]
    fn model_settings(&self) -> &ModelSettings;

    fn agent_settings(&self) -> &AgentSettings;
    async fn get_channel_sender(&self) -> crate::Result<Sender<AgentRequestPkg>>;
}
#[async_trait]
pub trait Agent: SessionCompactSupport + AgentClone + AgentVisitor + Send + Sync {
    async fn start(self: Arc<Self>) -> crate::Result<Arc<dyn Agent>>;

    async fn fork_agent(
        &self,
        agent_id: &AgentId,
        agent_group: &AgentGroup,
        addi_system_prompt: Option<SystemPrompt>,
        desc: Option<String>,
        owner_session: &OwnerSession,
    ) -> crate::Result<Arc<dyn Agent>> {
        if self.id().eq(agent_id) || self.agent_group().eq(agent_group) {
            return Err(anyhow!(
                "fork child with agent_group: {agent_group} is forbidden"
            ));
        }
        let forked = self
            .context()
            .agent_registry
            .get_with(
                self.context(),
                agent_id,
                |context, agent_id| async move {
                    let agent = if let Ok(agent) = reload_agent(&agent_id, context).await {
                        agent
                    } else {
                        spawn_agent(
                            &agent_id,
                            agent_group,
                            addi_system_prompt,
                            desc,
                            &owner_session,
                            context,
                        )
                        .await?
                    };
                    Ok::<_, anyhow::Error>(agent)
                },
            )
            .await?;
        Ok(forked)
    }
}

#[async_trait]
pub trait AgentClone: Send + Sync {
    async fn clone_with(
        &self,
        id: AgentId,
        agent_settings: Option<AgentSettings>,
    ) -> crate::Result<Arc<dyn Agent>>;
}

mod agent_context;
pub use agent_context::*;

mod agent_;
pub use agent_::*;

mod agent_request;
pub use agent_request::*;

mod agent_response;
pub use agent_response::*;

mod session_compact;
pub use session_compact::*;
mod agent_settings;
pub use agent_settings::*;

mod agent_factory;
pub use agent_factory::*;

mod main_agent;
pub use main_agent::MainAgent;

pub trait DelegatedAgent {
    fn delegated(&self) -> &Arc<dyn Agent>;
}

#[async_trait]
impl<A> SessionCompactSupport for A
where
    A: DelegatedAgent + Send + Sync,
{
    async fn session_compact(
        self: Arc<Self>,
        channel_message_sender: Sender<crate::Result<ChannelMessage>>,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> HistoryCompactResult {
        Arc::clone(&self.delegated())
            .session_compact(channel_message_sender, session_id, compact_ratio)
            .await
    }
}

#[async_trait]
impl<A> AgentClone for A
where
    A: DelegatedAgent + Send + Sync,
{
    async fn clone_with(
        &self,
        id: AgentId,
        agent_settings: Option<AgentSettings>,
    ) -> crate::Result<Arc<dyn Agent>> {
        Arc::clone(&self.delegated())
            .clone_with(id, agent_settings)
            .await
    }
}

#[async_trait]
impl<A> AgentVisitor for A
where
    A: DelegatedAgent + Send + Sync,
{
    fn id(&self) -> &AgentId {
        self.delegated().id()
    }

    fn context(&self) -> &'static AgentContext {
        self.delegated().context()
    }

    fn agent_group(&self) -> &AgentGroup {
        self.delegated().agent_group()
    }

    fn description(&self) -> &str {
        self.delegated().description()
    }

    fn owner_session(&self) -> &OwnerSession {
        self.delegated().owner_session()
    }

    fn model_settings(&self) -> &ModelSettings {
        self.delegated().model_settings()
    }

    fn agent_settings(&self) -> &AgentSettings {
        self.delegated().agent_settings()
    }

    async fn get_channel_sender(&self) -> crate::Result<Sender<AgentRequestPkg>> {
        self.delegated().get_channel_sender().await
    }
}

#[async_trait]
impl<A> Agent for A
where
    A: DelegatedAgent + Send + Sync + 'static,
{
    async fn start(self: Arc<Self>) -> crate::Result<Arc<dyn Agent>> {
        let _ = self.delegated().clone().start().await?;
        Ok(self)
    }
}
