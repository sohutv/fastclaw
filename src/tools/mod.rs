use crate::agent::{Agent, AgentContext};
use crate::channels::{ChannelMessage, SessionId};
use derive_more::From;
use rig::tool::ToolDyn;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
mod shell_tool;
mod task_tool;
pub use task_tool::{TaskSchedule, TaskTools};

mod time_tool;

mod websearch_tool;

mod image_tool;
mod media;
use media::*;

#[cfg(feature = "cloud_storage_tool")]
mod cloud_storage_tool;
mod memory_recall;

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
    pub session_id: SessionId,
    pub agent: Arc<dyn Agent>,
    #[allow(unused)]
    pub channel_message_sender: Sender<ChannelMessage>,
}

impl ToolContext {
    fn agent_context(&self) -> &AgentContext {
        self.agent.context()
    }
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
            if let Some(_) = ctx.agent_context().config.websearch {
                vec![Box::new(websearch_tool::WebSearchTool { ctx: ctx.clone() })]
            } else {
                vec![]
            },
            image_tool::ImageTools::create(ctx.clone()).await?,
            #[cfg(feature = "cloud_storage_tool")]
            if let Some(_) = ctx.agent_context().config.storage {
                cloud_storage_tool::CloudStorageTools::create(ctx.clone()).await?
            } else {
                vec![]
            },
            TaskTools::create(ctx.clone()).await?,
            if let Some(_) = ctx.agent_context().config.embedding {
                vec![Box::new(memory_recall::MemoryRecallTool { ctx: ctx.clone() })]
            } else {
                vec![]
            },
        ];
        Ok(tools.into_iter().flatten().collect())
    }
}
