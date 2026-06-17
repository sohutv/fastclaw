use crate::agent::{Agent, AgentId, DelegatedAgent, reload_agent, AgentVisitor};
use anyhow::anyhow;
use std::ops::Deref;
use std::sync::Arc;

#[derive(Clone)]
pub struct MainAgent {
    delegated: Arc<dyn Agent>,
}

impl MainAgent {
    pub async fn new(delegated: Arc<dyn Agent>) -> crate::Result<Self> {
        if delegated.id().is_main() {
            Ok(Self { delegated })
        } else {
            Err(anyhow!("not main agent"))
        }
    }
}

impl DelegatedAgent for MainAgent {
    fn delegated(&self) -> &Arc<dyn Agent> {
        &self.delegated
    }
}

impl Deref for MainAgent {
    type Target = dyn Agent;

    fn deref(&self) -> &Self::Target {
        self.delegated.deref()
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
                                    &agent_id,
                                    self.context(),
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
