use crate::tools::ToolContext;
use rig::tool::ToolDyn;
use serde::{Deserialize, Serialize};

pub mod fork_child_agent;

pub mod drop_child_agent;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DaemonAgentToolsConfig {
    pub fork_enable: bool,
    pub drop_enable: bool,
}

#[derive(Clone)]
pub struct DaemonAgentTools;

impl DaemonAgentTools {
    pub async fn create(ctx: ToolContext) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        Ok(vec![
            Box::new(fork_child_agent::ForkChildAgentTool { ctx: ctx.clone() }),
            Box::new(drop_child_agent::DropChildAgentTool { ctx: ctx.clone() }),
        ])
    }
}
