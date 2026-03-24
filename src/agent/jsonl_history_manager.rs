use crate::agent::{AgentName, HistoryManager, Workspace};
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
        agent: &AgentName,
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

    async fn load(&self, session_id: &SessionId, agent: &AgentName) -> crate::Result<Vec<Message>> {
        let dir = self.history_dir(session_id, agent).await?;
        let filepath = dir.join("history.jsonl");
        if !filepath.exists() {
            return Ok(Vec::new());
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
        Ok(messages)
    }

    async fn usage(&self, session_id: &SessionId, agent: &AgentName) -> crate::Result<Usage> {
        let dir = self.history_dir(session_id, agent).await?;
        let usage_filepath = dir.join("usage.json");
        if !usage_filepath.exists() {
            return Ok(Default::default());
        }
        let json = fs::read_to_string(&usage_filepath).await?;
        Ok(serde_json::from_str(&json).unwrap_or_default())
    }
}

impl JsonlHistoryManager {
    async fn history_dir(
        &self,
        session_id: &SessionId,
        agent: &AgentName,
    ) -> crate::Result<PathBuf> {
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
