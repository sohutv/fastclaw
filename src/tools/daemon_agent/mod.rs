use crate::tools::ToolContext;
use rig::tool::ToolDyn;
use serde::{Deserialize, Serialize};

pub mod fork_child_agent;

pub mod drop_child_agent;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DaemonAgentToolsConfig {
    pub enable: bool,
}

#[derive(Clone)]
pub struct DaemonAgentTools;

impl DaemonAgentTools {
    pub async fn create(ctx: ToolContext) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        let DaemonAgentToolsConfig { enable } =
            ctx.config.daemon_agent_tools.clone().unwrap_or_default();

        let mut arr: Vec<Box<dyn ToolDyn>> = vec![];
        if enable {
            arr.push(Box::new(fork_child_agent::ForkChildAgentTool {
                ctx: ctx.clone(),
            }));
            arr.push(Box::new(drop_child_agent::DropChildAgentTool {
                ctx: ctx.clone(),
            }));
        }
        Ok(arr)
    }
}
