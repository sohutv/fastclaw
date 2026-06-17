use std::fmt::Display;
use std::sync::Arc;
use async_trait::async_trait;
use rig::completion::Usage;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;
use crate::channels::{ChannelMessage, SessionId};

#[async_trait]
pub trait SessionCompactSupport: Send + Sync {
    async fn session_compact(
        self: Arc<Self>,
        channel_message_sender: Sender<crate::Result<ChannelMessage>>,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> HistoryCompactResult;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HistoryCompactResult {
    Ok(HistoryCompactVal),
    Err(String),
    Ignore(String),
}

impl<Err: std::fmt::Display> From<Err> for HistoryCompactResult {
    fn from(value: Err) -> Self {
        HistoryCompactResult::Err(value.to_string())
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HistoryCompactVal {
    current: Usage,
    before: Usage,
    compact_ratio: f64,
}

impl HistoryCompactVal {
    pub fn new(before: Usage, after: Usage) -> Self {
        Self {
            current: after,
            before,
            compact_ratio: (1. - (after.total_tokens as f64 / before.total_tokens as f64)) * 100.,
        }
    }

    pub fn current(&self) -> &Usage {
        &self.current
    }

    pub fn before(&self) -> &Usage {
        &self.before
    }

    pub fn compact_ratio(&self) -> f64 {
        self.compact_ratio
    }
}

impl Display for HistoryCompactVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "total usage {} -> {}, compression ratio: {:.2}%",
            self.before.total_tokens, self.current.total_tokens, self.compact_ratio
        )
    }
}