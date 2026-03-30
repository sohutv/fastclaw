use crate::agent::AgentId;
use crate::channels::SessionId;
use async_trait::async_trait;
use derive_more::{Deref, Display};
use rig::completion::{Message, Usage};
use serde::{Deserialize, Serialize};
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

    async fn load(
        &self,
        session_id: &SessionId,
        agent: &AgentId,
    ) -> crate::Result<(Vec<Message>, Usage)>;

    #[allow(unused)]
    async fn usage(&self, session_id: &SessionId, agent: &AgentId) -> crate::Result<Usage>;

    async fn backup(
        &mut self,
        session_id: &SessionId,
        agent: &AgentId,
    ) -> crate::Result<(PathBuf, BackupTimestamp)>;
}

#[derive(Debug, Clone, Deref, Serialize, Deserialize, Display)]
pub struct BackupTimestamp(String);
