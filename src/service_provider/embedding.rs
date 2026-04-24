use crate::config::Workspace;
use async_trait::async_trait;
use derive_more::{Deref, Display, From};
use itertools::Itertools;
use rig::agent::Text;
use rig::completion::{AssistantContent, Message};
use rig::message::{ToolResult, ToolResultContent, UserContent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EmbeddingConfigs {
    #[cfg(feature = "volcengine")]
    #[serde(rename = "volcengine")]
    Volcengine(super::volcengine::embedding::VolcengineEmbeddingConfig),
}

impl EmbeddingConfigs {
    pub async fn try_into_embedding(&self) -> crate::Result<Arc<dyn Embedding>> {
        match self {
            #[cfg(feature = "volcengine")]
            EmbeddingConfigs::Volcengine(config) => {
                let embedding = config.try_into_embedding().await?;
                Ok(Arc::new(embedding))
            }
        }
    }
}

pub type Id = uuid::Uuid;
#[derive(Debug, Clone)]
pub struct EmbeddingArgs {
    pub resources: HashMap<Id, EmbeddingResources>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, Deref, Eq, PartialEq, Ord, PartialOrd, Hash, Display,
)]
pub enum EmbeddingResource {
    Text(crate::Text),
}

impl From<String> for EmbeddingResource {
    fn from(value: String) -> Self {
        Self::Text(value.into())
    }
}

impl From<&String> for EmbeddingResource {
    fn from(value: &String) -> Self {
        value.clone().into()
    }
}

impl From<&str> for EmbeddingResource {
    fn from(value: &str) -> Self {
        value.to_string().into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Deref)]
pub struct EmbeddingResources(Vec<EmbeddingResource>);

impl Display for EmbeddingResources {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let text = self.iter().join("\n");
        write!(f, "{}", text)
    }
}

impl<R> From<R> for EmbeddingResources
where
    R: Into<EmbeddingResource>,
{
    fn from(value: R) -> Self {
        Self(vec![value.into()])
    }
}

impl TryFrom<&Message> for EmbeddingResources {
    type Error = anyhow::Error;

    fn try_from(value: &Message) -> Result<Self, Self::Error> {
        value.clone().try_into()
    }
}
impl TryFrom<Message> for EmbeddingResources {
    type Error = anyhow::Error;

    fn try_from(value: Message) -> crate::Result<Self> {
        let array = match value {
            Message::System { content, .. } => vec![content.into()],
            Message::User { content, .. } => content
                .iter()
                .flat_map(|it| match it {
                    UserContent::Text(Text { text, .. }) => vec![text.into()],
                    UserContent::ToolResult(ToolResult { content, .. }) => content
                        .iter()
                        .flat_map(|it| match it {
                            ToolResultContent::Text(Text { text, .. }) => Some(text.into()),
                            ToolResultContent::Image(_) => None,
                        })
                        .collect_vec(),
                    _ => vec![],
                })
                .collect_vec(),
            Message::Assistant { content, .. } => content
                .iter()
                .flat_map(|it| match it {
                    AssistantContent::Text(Text { text, .. }) => vec![text.into()],
                    _ => vec![],
                })
                .collect_vec(),
        };
        Ok(Self(array))
    }
}

#[derive(Debug, Clone, Deref)]
pub struct EmbeddingResult {
    #[deref]
    pub vectors: HashMap<Id, Vector>,
}

#[derive(Debug, Clone, Deref, Display)]
#[display("{vector:?}")]
pub struct Vector {
    pub resources: EmbeddingResources,
    #[deref]
    pub vector: Vector_,
}

#[derive(Debug, Clone, Deref, Display, From)]
#[display("{_0:?}")]
pub struct Vector_(Vec<f32>);

impl Vector_ {
    pub fn as_bytes(&self) -> Vec<u8> {
        bytemuck::cast_slice(&self).to_vec()
    }
}

#[async_trait]
pub trait Embedding: Sync + Send {
    async fn embedding(
        &self,
        workspace: &'static Workspace,
        args: EmbeddingArgs,
    ) -> crate::Result<EmbeddingResult>;
}

#[async_trait]
pub trait EmbeddingConfig: Sync + Send {
    type T: Embedding;
    async fn try_into_embedding(&self) -> crate::Result<Self::T>;
}
