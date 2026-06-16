use crate::agent::{
    Agent, AgentClone, AgentContext, AgentGroup, AgentId, AgentRequestPkg, AgentSettings,
    HistoryCompactResult, OwnerSession, SessionCompactSupport, reload_agent,
};
use crate::channels::{ChannelMessage, SessionId};
use crate::model_provider::ModelSettings;
use anyhow::anyhow;
use async_trait::async_trait;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

#[derive(Clone)]
pub struct MainAgent(Arc<dyn Agent>);

impl Deref for MainAgent {
    type Target = dyn Agent;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl TryFrom<Arc<dyn Agent>> for MainAgent {
    type Error = anyhow::Error;

    fn try_from(value: Arc<dyn Agent>) -> Result<Self, Self::Error> {
        if value.id().is_main() {
            Ok(Self(value))
        } else {
            Err(anyhow!("not main agent"))
        }
    }
}

#[async_trait]
impl SessionCompactSupport for MainAgent {
    async fn session_compact(
        self: Arc<Self>,
        channel_message_sender: Sender<crate::Result<ChannelMessage>>,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> HistoryCompactResult {
        Arc::clone(&self.0)
            .session_compact(channel_message_sender, session_id, compact_ratio)
            .await
    }
}

#[async_trait]
impl AgentClone for MainAgent {
    async fn clone_with(
        &self,
        id: AgentId,
        agent_settings: Option<AgentSettings>,
    ) -> crate::Result<Arc<dyn Agent>> {
        Arc::clone(&self.0).clone_with(id, agent_settings).await
    }
}

#[async_trait]
impl Agent for MainAgent {
    async fn start(self: Arc<Self>) -> crate::Result<Arc<dyn Agent>> {
        Arc::clone(&self.0).start().await
    }

    async fn get_channel_sender(&self) -> crate::Result<Sender<AgentRequestPkg>> {
        self.0.get_channel_sender().await
    }

    fn context(&self) -> &AgentContext {
        self.0.context()
    }

    fn model_settings(&self) -> &ModelSettings {
        self.0.model_settings()
    }

    fn agent_settings(&self) -> &AgentSettings {
        self.0.agent_settings()
    }

    fn id(&self) -> &AgentId {
        self.0.id()
    }

    fn agent_context(&self) -> Arc<AgentContext> {
        self.0.agent_context()
    }

    fn agent_group(&self) -> &AgentGroup {
        self.0.agent_group()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn owner_session(&self) -> &OwnerSession {
        self.0.owner_session()
    }
}

impl MainAgent {
    pub async fn init_children(self) -> crate::Result<Self> {
        let workspace = self.context().workspace;
        if let Ok(path) = workspace.agent_group_agents_path().await {
            if let Ok(mut dir) = tokio::fs::read_dir(&path).await {
                while let Ok(Some(dir_entry)) = dir.next_entry().await {
                    if let Ok(agent_id_str) = dir_entry.file_name().into_string() {
                        let agent_id = AgentId::from(agent_id_str);
                        if let Ok(agent_lock_path) =
                            workspace.agent_group_agent_lock_path(&agent_id).await
                        {
                            if agent_lock_path.exists() {
                                let _ = reload_agent(
                                    self.context().config,
                                    &self.context().history_manager,
                                    &self.context().memory_manager,
                                    workspace,
                                    self.context().mcp_registry,
                                    &agent_id,
                                )
                                .await;
                            }
                        }
                    }
                }
            }
        }
        Ok(self)
    }
}
