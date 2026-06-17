use rig::completion::{Message, Usage};
use rig::message::{Reasoning, ToolCall};
use serde::{Deserialize, Serialize};
use crate::agent::HistoryCompactResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentResponse {
    Start,
    ToolCall(ToolCall),
    ReasoningStream(Reasoning),
    MessageStream(Message),
    Final(Usage),
    Error(String),
    Notify(Notify),
    HistoryCompact(HistoryCompactResult),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Notify {
    Text(String),
    Markdown { title: String, content: String },
}

impl<S: Into<String>> From<S> for Notify {
    fn from(value: S) -> Self {
        Notify::Text(value.into())
    }
}
