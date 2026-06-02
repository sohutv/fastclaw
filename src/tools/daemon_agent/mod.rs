use crate::tools::ToolContext;
use rig::tool::ToolDyn;

pub mod fork_child_agent;

pub mod drop_child_agent;

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
