use std::sync::Arc;
use crate::agent::AgentContext;

#[derive(Clone)]
pub struct TaskCreateTool {
    ctx: Arc<AgentContext>,
}

