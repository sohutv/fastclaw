use crate::tools::ToolContext;
use rig::tool::ToolDyn;

mod imagegen_tool;

mod image_understanding;

pub use image_understanding::Config as ImageUnderstandingConfig;

mod image_enhancer;

#[derive(Clone)]
pub struct ImageTools;

impl ImageTools {
    pub async fn create(ctx: ToolContext) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        let mut tools = vec![];
        if ctx.config.image_understanding.enable {
            tools.push(
                Box::new(image_understanding::ImageUnderstandingTool { ctx: ctx.clone() })
                    as Box<dyn ToolDyn>,
            );
        }
        if ctx.config.imagegen.is_some() {
            tools.push(
                Box::new(imagegen_tool::ImageGenTool { ctx: ctx.clone() }) as Box<dyn ToolDyn>
            );
        }
        if ctx.config.image_enhancer.is_some() {
            tools.push(
                Box::new(image_enhancer::ImageEnhancerTool { ctx: ctx.clone() })
                    as Box<dyn ToolDyn>,
            );
        }
        Ok(tools)
    }
}
