use crate::agent::session_history::{HistoryMessage, StoreOption};
use crate::agent::{AgentId, HistoryManager, Workspace};
use crate::channels::SessionId;
use crate::config::Config;
use anyhow::anyhow;
use async_trait::async_trait;
use rig::completion::Usage;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tokio::{
    fs,
    io::{AsyncBufReadExt, BufReader},
};

#[derive(Clone)]
pub struct JsonlHistoryManager {
    #[allow(unused)]
    config: &'static Config,
    workspace: &'static Workspace,
    histories: Arc<RwLock<Option<(Vec<HistoryMessage>, Usage)>>>,
}

impl JsonlHistoryManager {
    pub async fn new(
        config: &'static Config,
        workspace: &'static Workspace,
    ) -> crate::Result<Self> {
        Ok(Self {
            config,
            workspace,
            histories: Default::default(),
        })
    }
}

#[async_trait]
impl HistoryManager for JsonlHistoryManager {
    async fn store(
        &self,
        session_id: &SessionId,
        agent: &AgentId,
        usage: &Usage,
        update_messages: Vec<HistoryMessage>,
        option: StoreOption,
    ) -> crate::Result<()> {
        let mut histories = self.histories.write().await;

        {
            // dump to file
            let dir = self.history_dir(session_id, agent).await?;
            let usage_filepath = dir.join("usage.json");
            fs::write(
                &usage_filepath,
                serde_json::to_string_pretty(usage).unwrap_or_default(),
            )
            .await?;
            let history_filepath = dir.join("history.jsonl");

            match option {
                StoreOption::Append => {
                    let file = fs::File::options()
                        .create(true)
                        .write(true)
                        .append(true)
                        .open(&history_filepath)
                        .await?;
                    let _ = Self::write_actual(file, &update_messages).await?;
                }
                StoreOption::Overwrite => {
                    let tmp_filepath = history_filepath
                        .parent()
                        .ok_or(anyhow!(
                            "unexpected history filepath: {}",
                            history_filepath.display()
                        ))?
                        .join(format!(
                            "tmp_{}_{}.jsonl",
                            chrono::Local::now().format("%Y%m%d_%H%M%S"),
                            uuid::Uuid::new_v4(),
                        ));
                    let file = fs::File::options()
                        .create(true)
                        .write(true)
                        .open(&tmp_filepath)
                        .await?;
                    let _ = Self::write_actual(file, &update_messages).await?;
                    fs::rename(&tmp_filepath, &history_filepath).await?;
                }
            }
        }

        if let Some((messages, usage)) = histories.deref_mut() {
            for update_message in update_messages {
                match update_message {
                    HistoryMessage::Message(_) => {
                        messages.push(update_message);
                    }
                    HistoryMessage::Summary(_) => {
                        *messages = vec![];
                        messages.push(update_message);
                    }
                }
            }
            *usage = *usage;
        }

        Ok(())
    }

    async fn load(
        &self,
        session_id: &SessionId,
        agent: &AgentId,
    ) -> crate::Result<(Vec<HistoryMessage>, Usage)> {
        {
            let histories = self.histories.read().await;
            if let Some((messages, usage)) = histories.as_ref() {
                return Ok((messages.clone(), *usage));
            }
        }
        {
            let mut histories = self.histories.write().await;
            *histories = {
                let dir = self.history_dir(session_id, agent).await?;
                let reader = {
                    let filepath = dir.join("history.jsonl");
                    if !filepath.exists() {
                        return Ok((Default::default(), Default::default()));
                    }
                    let file = fs::File::open(&filepath).await?;
                    BufReader::new(file)
                };
                let mut messages = Vec::new();
                let mut lines = reader.lines();
                while let Some(line) = lines.next_line().await? {
                    if let Ok(message) = serde_json::from_str::<HistoryMessage>(&line) {
                        match message {
                            HistoryMessage::Message(_) => {
                                messages.push(message);
                            }
                            HistoryMessage::Summary(_) => {
                                messages = vec![];
                                messages.push(message);
                            }
                        }
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
                Some((messages, usage))
            };
        }
        self.load(session_id, agent).await
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
}

impl JsonlHistoryManager {
    async fn history_dir(&self, session_id: &SessionId, agent: &AgentId) -> crate::Result<PathBuf> {
        let dir = self.workspace.session_path(session_id).join(agent.deref());
        if !dir.exists() {
            fs::create_dir_all(&dir).await?;
        }
        if !dir.is_dir() {
            return Err(anyhow::anyhow!("{} is not a directory", dir.display()));
        }
        Ok(dir)
    }

    async fn write_actual(file: fs::File, messages: &[HistoryMessage]) -> crate::Result<()> {
        let mut writer = tokio::io::BufWriter::new(file);
        for message in messages {
            let line = serde_json::to_string(message)?;
            let _ = writer.write(line.as_bytes()).await?;
            let _ = writer.write(b"\n").await?;
        }
        let _ = writer.flush().await?;
        Ok(())
    }
}
