use crate::agent::AgentId;
use crate::channels::SessionId;
use async_trait::async_trait;
use derive_more::Deref;
use rig::completion::{Message, Usage};
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};

mod jsonl_history_manager;
pub use jsonl_history_manager::JsonlHistoryManager;

#[async_trait]
pub trait HistoryManager: Send + Sync {
    async fn append(
        &self,
        session_id: &SessionId,
        agent: &AgentId,
        usage: &Usage,
        message: Vec<HistoryMessage>,
        overwrite: Option<bool>,
    ) -> crate::Result<()>;

    async fn load(
        &self,
        session_id: &SessionId,
        agent: &AgentId,
    ) -> crate::Result<(Vec<HistoryMessage>, Usage)>;

    #[allow(unused)]
    async fn usage(&self, session_id: &SessionId, agent: &AgentId) -> crate::Result<Usage>;
}

#[derive(Debug, Clone, Serialize, Deref)]
#[serde(tag = "type")]
pub enum HistoryMessage {
    #[serde(rename = "message")]
    Message(Message),
    #[serde(rename = "summary")]
    Summary(Message),
}

impl<'de> Deserialize<'de> for HistoryMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json = serde_json::value::Value::deserialize(deserializer)?;
        let type_ = if let Some(type_) = json.get("type").and_then(|it| it.as_str()) {
            type_
        } else {
            "message"
        };
        match type_ {
            "message" => Ok(HistoryMessage::message(
                serde_json::from_value::<Message>(json)
                    .map_err(|err| D::Error::custom(format!("{err}")))?,
            )),
            "summary" => Ok(HistoryMessage::summary(
                serde_json::from_value::<Message>(json)
                    .map_err(|err| D::Error::custom(format!("{err}")))?,
            )),
            _ => Err(D::Error::custom(format!("unexpected type: {}", type_))),
        }
    }
}

impl HistoryMessage {
    pub fn message<M: Into<Message>>(message: M) -> Self {
        Self::Message(message.into())
    }
    pub fn summary<M: Into<Message>>(message: M) -> Self {
        Self::Summary(message.into())
    }

    pub fn is_message(&self) -> bool {
        match self {
            HistoryMessage::Message(_) => true,
            _ => false,
        }
    }
}

impl From<HistoryMessage> for Message {
    fn from(value: HistoryMessage) -> Self {
        match value {
            HistoryMessage::Message(m) => m,
            HistoryMessage::Summary(m) => m,
        }
    }
}
