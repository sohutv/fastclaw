use crate::tools::tool_filter::ToolNameFilter;
use derive_more::Deref;
use rig::providers::openai::responses_api::ReasoningEffort;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub max_tokens: Option<u64>,
    pub temperature: f64,
    pub max_turns: usize,
    pub reasoning_effort: ReasoningEffort,
    pub compact_threshold: f32,
    pub task_queue_size: TaskQueueSize,
    pub task_backpressure: TaskBackpressure,
    pub chat_history_limit: Option<usize>,
    pub history_compact_enable: bool,
    pub tool_filter: Option<ToolNameFilter>,
    pub output_schema: Option<schemars::Schema>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Deref)]
pub struct TaskQueueSize(usize);
impl Default for TaskQueueSize {
    fn default() -> Self {
        Self(8)
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum TaskBackpressure {
    #[default]
    #[serde(alias = "pending")]
    Pending,
    #[serde(alias = "latest", alias = "drop")]
    Latest,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_tokens: None,
            temperature: 1.,
            max_turns: 256,
            compact_threshold: 0.8,
            reasoning_effort: Default::default(),
            task_queue_size: Default::default(),
            task_backpressure: Default::default(),
            chat_history_limit: None,
            history_compact_enable: true,
            tool_filter: None,
            output_schema: None,
        }
    }
}
