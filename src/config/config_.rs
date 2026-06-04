use crate::ModelName;
use crate::agent::AgentGroup;
use crate::config::{AgentSettings, Config};
use crate::model_provider::{ModelProviderName, ModelProviders};
use anyhow::anyhow;
use std::path::{Path, PathBuf};

impl Config {
    pub fn model_provider(&self, name: &ModelProviderName) -> crate::Result<ModelProviders> {
        if let Some(provider) = self.model_providers.get(name).map(|it| it.clone()) {
            Ok(provider)
        } else {
            Err(anyhow!("Model provider not found for name: {}", name))
        }
    }

    pub fn default_model_provider(&self) -> crate::Result<ModelProviders> {
        self.model_provider(&self.default_model_provider)
    }

    pub fn default_model(&self) -> &ModelName {
        &self.default_model
    }

    pub fn init_logger<P: AsRef<Path>>(&mut self, workdir: P) -> crate::Result<&mut Self> {
        self.log_config.init(workdir)?;
        Ok(self)
    }

    pub fn agent_settings(&self, group: &AgentGroup) -> Option<&AgentSettings> {
        self.agent_settings.get(group)
    }
}

impl Config {
    pub fn default_workdir() -> PathBuf {
        let user_dirs = directories::UserDirs::new().expect("user home not exist!!!");
        user_dirs.home_dir().join(".fastclaw")
    }
}
