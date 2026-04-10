use crate::config::Workspace;
use async_trait::async_trait;
use derive_more::From;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StorageConfigs {
    #[cfg(feature = "volcengine")]
    #[serde(rename = "volcengine")]
    Volcengine(super::volcengine::storage::VolcengineStorageConfig),
}

impl StorageConfigs {
    pub async fn try_into_storage(&self) -> crate::Result<Arc<dyn Storage>> {
        match self {
            #[cfg(feature = "volcengine")]
            StorageConfigs::Volcengine(config) => {
                let storage = config.try_into_storage().await?;
                Ok(Arc::new(storage))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoreArgs {
    pub key: ObjectKey,
    pub mime: mime::Mime,
    pub content: Content,
}

#[derive(Debug, Clone, From)]
pub enum Content {
    Url(Url),
    File(PathBuf),
    String(String),
    Raw(Vec<u8>),
}

impl Content {
    pub async fn into_bytes(self) -> crate::Result<Vec<u8>> {
        match self {
            Content::Url(url) => Ok(reqwest::get(url).await?.bytes().await?.to_vec()),
            Content::File(path) => Ok(tokio::fs::read(&path).await?),
            Content::String(string) => Ok(string.into_bytes()),
            Content::Raw(bytes) => Ok(bytes),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoreResult {
    pub key: ObjectKey,
    pub request_id: String,
    pub signed_url: Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadArgs {
    pub key: ObjectKey,
}

impl<K: Into<ObjectKey>> From<K> for LoadArgs {
    fn from(key: K) -> Self {
        Self { key: key.into() }
    }
}

#[derive(Debug, Clone)]
pub struct LoadResult {
    pub key: ObjectKey,
    pub content: Vec<u8>,
    pub md5: String,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelArgs {
    pub key: ObjectKey,
}

impl<K: Into<ObjectKey>> From<K> for DelArgs {
    fn from(key: K) -> Self {
        Self { key: key.into() }
    }
}

#[derive(Debug, Clone)]
pub struct DelResult {
    pub key: ObjectKey,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Ord, PartialOrd, PartialEq, Eq)]
pub struct ObjectKey(String);

impl<S: Display> From<S> for ObjectKey {
    fn from(value: S) -> Self {
        Self(value.to_string())
    }
}

impl ObjectKey {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[async_trait]
pub trait Storage: Sync + Send {
    async fn store(
        &self,
        workspace: &'static Workspace,
        args: StoreArgs,
    ) -> crate::Result<StoreResult>;

    async fn load(
        &self,
        workspace: &'static Workspace,
        args: LoadArgs,
    ) -> crate::Result<LoadResult>;

    async fn del(&self, workspace: &'static Workspace, args: DelArgs) -> crate::Result<DelResult>;
}

#[async_trait]
pub trait StorageConfig: Sync + Send {
    type T: Storage;
    async fn try_into_storage(&self) -> crate::Result<Self::T>;
}
