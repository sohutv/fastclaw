use crate::tools::ToolContext;
use rig::tool::ToolDyn;

mod del;
mod load;
mod store;

#[derive(Clone)]
pub struct CloudStorageTools;

impl CloudStorageTools {
    pub async fn create(ctx: ToolContext) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        Ok(vec![
            Box::new(store::CloudStorageStoreTool { ctx: ctx.clone() }),
            Box::new(load::CloudStorageLoadTool { ctx: ctx.clone() }),
            Box::new(del::CloudStorageDelTool { ctx: ctx.clone() }),
        ])
    }
}
