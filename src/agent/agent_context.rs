use crate::agent::{Agent, AgentId, HistoryManager};
use crate::channels::a2a_channel::A2AChannel;
use crate::config::{Config, Workspace};
use crate::memory::MemoryManager;
use crate::tools::mcp_tool::McpRegistry;
use crate::type_::SystemPrompt;
use anyhow::anyhow;
use async_trait::async_trait;
use derive_more::Deref;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(unused)]
#[derive(Clone)]
pub struct AgentContext {
    pub config: &'static Config,
    pub workspace: &'static Workspace,
    pub history_manager: Arc<dyn HistoryManager>,
    pub memory_manager: Arc<MemoryManager>,
    pub mcp_registry: &'static McpRegistry,
    pub agent_registry: &'static AgentRegistry,
    pub a2a_channel: &'static A2AChannel,
}

#[async_trait]
pub trait SystemPromptProvider: Send + Sync {
    async fn apply(&self) -> crate::Result<SystemPrompt>;
}

#[derive(Clone, Deref)]
pub struct AgentRegistry {
    #[allow(unused)]
    config: &'static Config,
    workspace: &'static Workspace,
    #[deref]
    container: Arc<RwLock<HashMap<AgentId, Arc<dyn Agent>>>>,
}

impl AgentRegistry {
    pub fn new(config: &'static Config, workspace: &'static Workspace) -> crate::Result<Self> {
        Ok(Self {
            config,
            workspace,
            container: Default::default(),
        })
    }

    pub async fn get_with<F, Fut>(
        &self,
        agent_context: &'static AgentContext,
        agent_id: &AgentId,
        agent_factory: F,
    ) -> crate::Result<Arc<dyn Agent>>
    where
        F: FnOnce(&'static AgentContext, AgentId) -> Fut,
        Fut: Future<Output = crate::Result<Arc<dyn Agent>>>,
    {
        let mut guard = self.write().await;
        if let Some(dst) = guard.get(agent_id) {
            Ok(Arc::clone(dst))
        } else {
            let dst = agent_factory(agent_context, agent_id.clone()).await?;
            let _ = tokio::fs::write(
                self.workspace
                    .agent_group_agent_lock_path(&agent_id)
                    .await?,
                format!("{}", chrono::Local::now().timestamp_millis()).as_bytes(),
            )
            .await;
            guard.entry(agent_id.clone()).or_insert(dst.clone());
            Ok(dst)
        }
    }

    pub async fn get(&self, agent_id: &AgentId) -> crate::Result<Arc<dyn Agent>> {
        let guard = self.read().await;
        let dst = guard.get(agent_id);
        dst.map(|it| Arc::clone(it))
            .ok_or(anyhow!("agent not exist, agent_id: {agent_id}"))
    }

    pub async fn drop(&self, agent_id: &AgentId) -> crate::Result<Arc<dyn Agent>> {
        let mut guard = self.write().await;
        if let Some(dst_agent) = guard.remove(agent_id) {
            return match tokio::fs::remove_file(
                self.workspace
                    .agent_group_agent_lock_path(&agent_id)
                    .await?,
            )
            .await
            {
                Ok(_) => Ok(dst_agent),
                Err(err) => {
                    guard.insert(agent_id.clone(), dst_agent);
                    Err(anyhow!("{err}"))
                }
            };
        }
        Err(anyhow!("agent not exist, agent_id: {agent_id}"))
    }
}
