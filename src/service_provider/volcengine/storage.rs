use crate::config::Workspace;
use crate::service_provider::{
    DelArgs, DelResult, LoadArgs, LoadResult, Storage, StorageConfig, StoreArgs, StoreResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolcengineStorageConfig {}

#[derive(Clone)]
pub struct VolcengineStorage {
    config: VolcengineStorageConfig,
}

#[async_trait]
impl StorageConfig for VolcengineStorageConfig {
    type T = VolcengineStorage;

    async fn try_into_storage(&self) -> crate::Result<Self::T> {
        todo!()
    }
}

#[async_trait]
impl Storage for VolcengineStorage {
    async fn store(
        &self,
        workspace: &'static Workspace,
        args: StoreArgs,
    ) -> crate::Result<StoreResult> {
        todo!()
    }

    async fn load(
        &self,
        workspace: &'static Workspace,
        args: LoadArgs,
    ) -> crate::Result<LoadResult> {
        todo!()
    }

    async fn del(&self, workspace: &'static Workspace, args: DelArgs) -> crate::Result<DelResult> {
        todo!()
    }
}

impl VolcengineStorageConfig {
    #[allow(unused)]
    fn from_env() -> crate::Result<Self> {
        todo!()
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_storage_workflow() -> crate::Result<()> {
        // store
        // load
        // del
        todo!()
    }
}
