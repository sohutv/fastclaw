use crate::agent::AgentContext;
use crate::tools::imagegen_tool::ImageGenTool;
use crate::tools::websearch_tool::WebSearchTool;
use derive_more::From;
use rig::tool::ToolDyn;
use serde::Serialize;
use std::sync::Arc;

mod memory_tool;
mod shell_tool;
mod task_tool;
pub(crate) use task_tool::TaskTools;
mod time_tool;

mod websearch_tool;

mod imagegen_tool;

#[cfg(feature = "cloud_storage_tool")]
mod cloud_storage_tool;

#[derive(Debug, Copy, Clone, serde::Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, thiserror::Error, From)]
#[error("{0}")]
pub struct ToolCallError(String);

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallRsult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl ToolCallRsult {
    fn ok(output: String) -> Self {
        Self {
            success: true,
            output,
            error: None,
        }
    }
    fn error<M: AsRef<str>>(msg: M) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(msg.as_ref().to_string()),
        }
    }
}

#[derive(Clone)]
pub struct FunctionTool;

impl FunctionTool {
    pub async fn required_tools(ctx: Arc<AgentContext>) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        let tools: Vec<Vec<Box<dyn ToolDyn>>> = vec![
            vec![Box::new(shell_tool::ShellTool::new(Arc::clone(&ctx))?)],
            vec![Box::new(time_tool::CurrentTimeTool)],
            if let Some(_) = ctx.config.websearch {
                vec![Box::new(WebSearchTool::new(Arc::clone(&ctx))?)]
            } else {
                vec![]
            },
            if let Some(_) = ctx.config.imagegen {
                vec![Box::new(ImageGenTool::new(Arc::clone(&ctx))?)]
            } else {
                vec![]
            },
            #[cfg(feature = "cloud_storage_tool")]
            if let Some(_) = ctx.config.storage {
                cloud_storage_tool::CloudStorageTools::create(Arc::clone(&ctx)).await?
            } else {
                vec![]
            },
            TaskTools::create(Arc::clone(&ctx)).await?,
        ];
        Ok(tools.into_iter().flatten().collect())
    }
}
