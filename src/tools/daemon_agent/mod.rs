use crate::tools::ToolContext;
use rig::tool::ToolDyn;

pub mod fork_agent;

#[derive(Clone)]
pub struct DaemonAgentTools;

impl DaemonAgentTools {
    pub async fn create(ctx: ToolContext) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        Ok(vec![])
    }
}
