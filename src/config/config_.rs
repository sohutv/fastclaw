use crate::config::Config;
use crate::model_provider::{ModelProviderName, ModelProviders};
use anyhow::anyhow;
use std::path::PathBuf;

impl Config {
    pub fn model_provider(&self, name: &ModelProviderName) -> crate::Result<ModelProviders> {
        if let Some(provider) = self.model_providers.get(name).map(|it| it.clone()) {
            Ok(provider)
        } else {
            Err(anyhow!("Model provider not found for name: {}", name))
        }
    }

    pub fn default_workdir() -> PathBuf {
        let user_dirs = directories::UserDirs::new().expect("user home not exist!!!");
        user_dirs.home_dir().join(".fastclaw")
    }
}
