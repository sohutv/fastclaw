use crate::config::Workspace;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreArgs {}

#[derive(Debug, Clone)]
pub struct StoreResult {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadArgs {}

#[derive(Debug, Clone)]
pub struct LoadResult {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelArgs {}

#[derive(Debug, Clone)]
pub struct DelResult {}

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
