//! volcengine embedding
//! https://www.volcengine.com/docs/82379/1409291?lang=zh
//!

use super::super::{Embedding, EmbeddingArgs, EmbeddingConfig, EmbeddingResult};
use crate::ModelName;
use crate::config::{ApiKey, ApiUrl, Workspace};
use crate::service_provider::{EmbeddingResource, EmbeddingResources, Vector};
use anyhow::anyhow;
use async_trait::async_trait;
use derive_more::{Deref, From};
use itertools::Itertools;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolcengineEmbeddingConfig {
    pub api_url: ApiUrl,
    pub api_key: ApiKey,
    pub model: ModelName,
}

#[derive(Clone)]
pub struct VolcengineEmbedding {
    config: VolcengineEmbeddingConfig,
}

#[async_trait]
impl EmbeddingConfig for VolcengineEmbeddingConfig {
    type T = VolcengineEmbedding;

    async fn try_into_embedding(&self) -> crate::Result<Self::T> {
        Ok(VolcengineEmbedding {
            config: self.clone(),
        })
    }
}

#[async_trait]
impl Embedding for VolcengineEmbedding {
    async fn embedding(
        &self,
        _: &'static Workspace,
        EmbeddingArgs { resources }: EmbeddingArgs,
    ) -> crate::Result<EmbeddingResult> {
        let mut vectors = HashMap::new();
        let resources = resources
            .into_iter()
            .filter(|(_, it)| !it.is_empty())
            .collect_vec();
        for (id, resources) in resources {
            let config = self.config.clone();
            match Self::embedding_actual(config, &resources).await {
                Ok(value) => {
                    vectors.insert(
                        id,
                        Vector {
                            resources,
                            vector: value.into(),
                        },
                    );
                }
                Err(err) => {
                    warn!("call embedding failed, err: {err}");
                }
            }
        }
        Ok(EmbeddingResult { vectors })
    }
}

impl VolcengineEmbedding {
    async fn embedding_actual(
        config: VolcengineEmbeddingConfig,
        resources: &EmbeddingResources,
    ) -> crate::Result<Vec<f32>> {
        let inputs = Inputs::try_from(resources)?;
        let response = reqwest::Client::default()
            .post(config.api_url.as_str())
            .header(
                "Authorization",
                format!("Bearer {}", config.api_key.as_str()),
            )
            .header("Content-Type", "application/json")
            .json(&json!({
                "model": config.model.as_str(),
                "dimensions": 1024,
                "encoding_format": "float",
                "input": inputs,
            }))
            .send()
            .await
            .map_err(|err| anyhow!(err))?
            .json::<Response>()
            .await
            .map_err(|err| anyhow!(err))?;
        match response {
            Response::Ok {
                data: Data { embedding, .. },
                ..
            } => Ok(embedding),
            Response::Err { code, message, .. } => Err(anyhow!(
                "exec embedding_actual failed, code: {}, message: {}",
                code,
                message.as_deref().unwrap_or("unknown")
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum Input<'a> {
    #[serde(rename = "text")]
    Text { text: &'a str },
}

#[derive(Debug, Clone, Serialize, From, Deref)]
struct Inputs<'a>(Vec<Input<'a>>);

impl<'a> TryFrom<&'a EmbeddingResources> for Inputs<'a> {
    type Error = anyhow::Error;

    fn try_from(values: &'a EmbeddingResources) -> Result<Self, Self::Error> {
        let inputs = values
            .iter()
            .map(|value| match value {
                EmbeddingResource::Text(text) => Input::Text { text },
            })
            .collect_vec();
        Ok(inputs.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Response {
    Ok {
        id: String,
        model: String,
        object: String,
        data: Data,
        usage: Usage,
        created: u64,
    },
    Err {
        code: String,
        message: Option<String>,
        #[serde(rename = "type")]
        type_: Option<String>,
        param: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    prompt_tokens: u64,
    prompt_tokens_details: HashMap<String, u64>,
    total_tokens: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Data {
    embedding: Vec<f32>,
    object: String,
}

impl VolcengineEmbeddingConfig {
    #[allow(unused)]
    fn from_env() -> crate::Result<Self> {
        Ok(Self {
            api_url: ApiUrl::from_str(std::env::var("VOLCENGINE_EMBEDDING_API_URL")?.as_str())?,
            api_key: std::env::var("VOLCENGINE_EMBEDDING_API_KEY")?.into(),
            model: std::env::var("VOLCENGINE_EMBEDDING_MODEL_NAME")?.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Workspace;
    use crate::hash_map;
    use crate::service_provider::volcengine::embedding::VolcengineEmbeddingConfig;
    use crate::service_provider::{Embedding, EmbeddingArgs, EmbeddingConfig, EmbeddingResult};

    #[tokio::test]
    async fn test_embedding() -> crate::Result<()> {
        let config = VolcengineEmbeddingConfig::from_env()?;
        let embedding = config.try_into_embedding().await?;
        let workspace: &'static Workspace = Box::leak(Box::new(Workspace::init("/tmp").await?));
        let EmbeddingResult { vectors, .. } = embedding
            .embedding(
                workspace,
                EmbeddingArgs {
                    resources: hash_map!(
                        uuid::Uuid::new_v4() => "一只阿拉蕾风格的兔子".into(),
                    ),
                },
            )
            .await?;
        for (id, vector) in vectors {
            println!("{} => {}", id, vector)
        }
        Ok(())
    }
}
