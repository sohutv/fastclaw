use crate::agent::AgentContext;
use crate::tools::websearch_tool::WebSearchTool;
use rig::tool::ToolDyn;
use serde::Serialize;
use std::sync::Arc;

mod memory_tool;
mod shell_tool;
mod task_tool;
mod time_tool;

mod websearch_tool;

#[derive(Debug, Copy, Clone, serde::Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, thiserror::Error)]
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
    pub fn required_tools(ctx: Arc<AgentContext>) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        let tools: Vec<Vec<Box<dyn ToolDyn>>> = vec![
            vec![Box::new(shell_tool::ShellTool::new(Arc::clone(&ctx))?)],
            vec![Box::new(time_tool::CurrentTimeTool)],
            if let Some(_) = ctx.config.websearch {
                vec![Box::new(WebSearchTool::new(Arc::clone(&ctx))?)]
            } else {
                vec![]
            },
            vec![Box::new(task_tool::TaskCreateTool::new(Arc::clone(&ctx))?)],
        ];
        Ok(tools.into_iter().flatten().collect())
    }
}
