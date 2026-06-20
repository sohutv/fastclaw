use crate::tools::ToolContext;
use rig::tool::ToolDyn;
use serde::{Deserialize, Serialize};

pub mod fork_agent;

pub mod drop_agent;

pub mod list_agent;

pub mod a2a_notifier;

pub mod a2user_notifier;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ForkAgentToolsConfig {
    pub enable: bool,
}

#[derive(Clone)]
pub struct ForkAgentTools;

impl ForkAgentTools {
    pub async fn create(ctx: ToolContext) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        let ForkAgentToolsConfig { enable } = ctx.config.fork_agent.clone().unwrap_or_default();

        let mut arr: Vec<Box<dyn ToolDyn>> = vec![];
        if enable {
            arr.push(Box::new(fork_agent::ForkAgentTool { ctx: ctx.clone() }));
            arr.push(Box::new(drop_agent::DropAgentTool { ctx: ctx.clone() }));
            arr.push(Box::new(list_agent::ListAgentsTool { ctx: ctx.clone() }));
            arr.push(Box::new(a2a_notifier::A2ANotifyTool { ctx: ctx.clone() }));
            arr.push(Box::new(a2user_notifier::A2UserNotifyTool {
                ctx: ctx.clone(),
            }));
        }
        Ok(arr)
    }
}
