use crate::agent::AgentContext;
use crate::channels::ChannelMessage;
use crate::tools::imagegen_tool::ImageGenTool;
use crate::tools::websearch_tool::WebSearchTool;
use derive_more::From;
use rig::tool::ToolDyn;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
mod shell_tool;
mod task_tool;
pub use task_tool::{TaskTools, TaskSchedule};

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

#[derive(Clone)]
pub struct ToolContext {
    pub agent_context: Arc<AgentContext>,
    pub channel_message_sender: Sender<ChannelMessage>,
}

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
    pub async fn required_tools(ctx: ToolContext) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        let tools: Vec<Vec<Box<dyn ToolDyn>>> = vec![
            vec![Box::new(shell_tool::ShellTool { ctx: ctx.clone() })],
            vec![Box::new(time_tool::CurrentTimeTool { ctx: ctx.clone() })],
            if let Some(_) = ctx.agent_context.config.websearch {
                vec![Box::new(WebSearchTool { ctx: ctx.clone() })]
            } else {
                vec![]
            },
            if let Some(_) = ctx.agent_context.config.imagegen {
                vec![Box::new(ImageGenTool { ctx: ctx.clone() })]
            } else {
                vec![]
            },
            #[cfg(feature = "cloud_storage_tool")]
            if let Some(_) = ctx.agent_context.config.storage {
                cloud_storage_tool::CloudStorageTools::create(ctx.clone()).await?
            } else {
                vec![]
            },
            TaskTools::create(ctx.clone()).await?,
        ];
        Ok(tools.into_iter().flatten().collect())
    }
}
