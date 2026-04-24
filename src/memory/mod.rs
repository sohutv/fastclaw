use crate::config::{Config, Workspace};
use crate::service_provider::EmbeddingConfigs;
use anyhow::anyhow;
use derive_more::{Deref, From};
use std::path::PathBuf;

mod embedding;

#[derive(Clone)]
pub struct MemoryManagerContext {
    #[allow(unused)]
    pub config: &'static Config,
    pub workspace: &'static Workspace,
    pub embedding_configs: EmbeddingConfigs,
}

pub struct MemoryManager {
    context: MemoryManagerContext,
}

impl MemoryManager {
    pub async fn new(
        config: &'static Config,
        workspace: &'static Workspace,
    ) -> crate::Result<Self> {
        let embedding_configs = config
            .embedding
            .as_ref()
            .ok_or(anyhow!("embedding config is required!!!"))?
            .clone();
        Ok(Self {
            context: MemoryManagerContext {
                config,
                workspace,
                embedding_configs,
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct SearchResultItem {
    pub id: usize,
    pub message: String,
    pub file_ref: Option<PathBuf>,
}

#[derive(Debug, Clone, Deref, From, Default)]
pub struct SearchResult(Vec<SearchResultItem>);
