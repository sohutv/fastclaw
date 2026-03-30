use crate::agent::session_history::BackupTimestamp;
use crate::agent::{AgentId, HistoryManager, Workspace};
use crate::channels::SessionId;
use async_trait::async_trait;
use itertools::Itertools;
use rig::completion::{Message, Usage};
use std::ops::Deref;
use std::path::PathBuf;
use tokio::{
    fs,
    io::{AsyncBufReadExt, BufReader},
};

#[derive(Clone)]
pub struct JsonlHistoryManager {
    workspace: &'static Workspace,
}

impl JsonlHistoryManager {
    pub async fn new(workspace: &'static Workspace) -> crate::Result<Self> {
        Ok(Self { workspace })
    }
}

#[async_trait]
impl HistoryManager for JsonlHistoryManager {
    async fn store(
        &mut self,
        session_id: &SessionId,
        agent: &AgentId,
        usage: &Usage,
        messages: &[Message],
    ) -> crate::Result<()> {
        let dir = self.history_dir(session_id, agent).await?;
        let usage_filepath = dir.join("usage.json");
        fs::write(
            &usage_filepath,
            serde_json::to_string_pretty(usage).unwrap_or_default(),
        )
        .await?;
        let history_filepath = dir.join("history.jsonl");
        let lines = messages
            .iter()
            .flat_map(|it| serde_json::to_string(&it).ok())
            .join("\n");
        fs::write(&history_filepath, lines).await?;
        Ok(())
    }

    async fn load(
        &self,
        session_id: &SessionId,
        agent: &AgentId,
    ) -> crate::Result<(Vec<Message>, Usage)> {
        let dir = self.history_dir(session_id, agent).await?;
        let filepath = dir.join("history.jsonl");
        if !filepath.exists() {
            return Ok((Default::default(), Default::default()));
        }
        let file = fs::File::open(&filepath).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut messages = Vec::new();
        while let Some(line) = lines.next_line().await? {
            if let Ok(message) = serde_json::from_str::<Message>(&line) {
                messages.push(message);
            }
        }
        let usage_filepath = dir.join("usage.json");
        let usage: Usage = if !usage_filepath.exists() {
            None
        } else {
            let json = fs::read_to_string(&usage_filepath).await?;
            serde_json::from_str::<Usage>(&json).ok()
        }
        .unwrap_or_default();
        Ok((messages.into_iter().collect(), usage))
    }

    async fn usage(&self, session_id: &SessionId, agent: &AgentId) -> crate::Result<Usage> {
        let dir = self.history_dir(session_id, agent).await?;
        let usage_filepath = dir.join("usage.json");
        if !usage_filepath.exists() {
            return Ok(Default::default());
        }
        let json = fs::read_to_string(&usage_filepath).await?;
        Ok(serde_json::from_str(&json).unwrap_or_default())
    }

    async fn backup(
        &mut self,
        session_id: &SessionId,
        agent: &AgentId,
    ) -> crate::Result<(PathBuf, BackupTimestamp)> {
        let dir = self.history_dir(session_id, agent).await?;
        let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S");
        let history_backup_path = dir.join(format!("history_{timestamp}.jsonl"));
        // backup usage
        let _ = tokio::fs::rename(
            &dir.join("usage.json"),
            &dir.join(format!("usage_{timestamp}.json")),
        )
        .await;
        let _ = tokio::fs::rename(&dir.join("history.jsonl"), &history_backup_path).await;
        Ok((
            history_backup_path
                .strip_prefix(&self.workspace.path)?
                .to_owned(),
            BackupTimestamp(timestamp.to_string()),
        ))
    }
}

impl JsonlHistoryManager {
    async fn history_dir(&self, session_id: &SessionId, agent: &AgentId) -> crate::Result<PathBuf> {
        let dir = self
            .workspace
            .path
            .join("sessions")
            .join(session_id.deref())
            .join(agent.deref());
        if !dir.exists() {
            fs::create_dir_all(&dir).await?;
        }
        if !dir.is_dir() {
            return Err(anyhow::anyhow!("{} is not a directory", dir.display()));
        }
        Ok(dir)
    }
}
