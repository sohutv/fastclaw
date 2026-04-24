use crate::tools::ToolContext;
use rig::tool::ToolDyn;

mod imagegen_tool;

#[cfg(feature = "tool_image_understanding")]
mod image_understanding;

mod image_enhancer;

#[derive(Clone)]
pub struct ImageTools;

impl ImageTools {
    pub async fn create(ctx: ToolContext) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        let mut tools = vec![];
        #[cfg(feature = "tool_image_understanding")]
        tools.push(
            Box::new(image_understanding::ImageUnderstandingTool { ctx: ctx.clone() })
                as Box<dyn ToolDyn>,
        );
        tools.push(Box::new(imagegen_tool::ImageGenTool { ctx: ctx.clone() }) as Box<dyn ToolDyn>);
        if ctx.agent_context().config.image_enhancer.is_some() {
            tools.push(Box::new(image_enhancer::ImageEnhancerTool { ctx: ctx.clone() }) as Box<dyn ToolDyn>);
        }
        Ok(tools)
    }
}
