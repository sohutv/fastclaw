use crate::agent::AgentId;
use crate::channels::SessionId;
use async_trait::async_trait;
use derive_more::Deref;
use rig::completion::{Message, Usage};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::PathBuf;

mod jsonl_history_manager;
pub use jsonl_history_manager::JsonlHistoryManager;

#[async_trait]
pub trait HistoryManager: Send + Sync {
    async fn store(
        &mut self,
        session_id: &SessionId,
        agent: &AgentId,
        usage: &Usage,
        message: &[Message],
    ) -> crate::Result<()>;

    async fn load_with_offset(
        &self,
        session_id: &SessionId,
        agent: &AgentId,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> crate::Result<(Vec<Message>, Usage)>;

    async fn load(
        &self,
        session_id: &SessionId,
        agent: &AgentId,
        limit: usize,
    ) -> crate::Result<Vec<Message>> {
        self.load_with_offset(session_id, agent, Some(0), Some(limit))
            .await
            .map(|(it, _)| it)
    }

    #[allow(unused)]
    async fn usage(&self, session_id: &SessionId, agent: &AgentId) -> crate::Result<Usage>;

    async fn backup(
        &mut self,
        session_id: &SessionId,
        agent: &AgentId,
    ) -> crate::Result<(PathBuf, BackupTimestamp)>;
}

#[derive(Debug, Clone, Deref, Serialize, Deserialize)]
pub struct BackupTimestamp(String);

impl<S: Display> From<S> for BackupTimestamp {
    fn from(value: S) -> Self {
        Self(value.to_string())
    }
}
