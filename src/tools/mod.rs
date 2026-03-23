use crate::agent::AgentContext;
use rig::tool::ToolDyn;
use serde::Serialize;
use std::sync::Arc;

mod reload_self_tool;
mod shell_tool;
mod task_callback_tool;

mod time_tool;

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

#[derive(Clone)]
pub struct FunctionTool;

impl FunctionTool {
    pub fn required_tools(ctx: Arc<AgentContext>) -> crate::Result<Vec<Box<dyn ToolDyn>>> {
        Ok(vec![
            Box::new(shell_tool::ShellTool::new(Arc::clone(&ctx))?),
            Box::new(reload_self_tool::ReloadSelfTool::new(Arc::clone(&ctx))?),
            Box::new(time_tool::CurrentTimeTool),
            Box::new(task_callback_tool::TaskCallbackTool::new(Arc::clone(&ctx))?),
        ])
    }
}
