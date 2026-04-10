use crate::agent::AgentContext;
use rig::tool::ToolDyn;
use std::sync::Arc;

mod del;
mod load;
mod store;

#[derive(Clone)]
pub struct CloudStorageTools;

impl CloudStorageTools {
    pub async fn create(ctx: Arc<AgentContext>) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        Ok(vec![
            Box::new(store::CloudStorageStoreTool::new(Arc::clone(&ctx))?),
            Box::new(load::CloudStorageLoadTool::new(Arc::clone(&ctx))?),
            Box::new(del::CloudStorageDelTool::new(Arc::clone(&ctx))?),
        ])
    }
}
