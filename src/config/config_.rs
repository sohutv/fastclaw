use crate::btree_map;
use crate::config::logger::LogConfig;
use crate::config::Config;
use crate::model_provider::{ModelName, ModelProviderName, ModelProviders};
use anyhow::anyhow;
use std::path::PathBuf;
use std::str::FromStr;

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

    pub fn init_logger(&mut self) -> crate::Result<&mut Self> {
        self.log_config.init()?;
        Ok(self)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_model_provider: Default::default(),
            default_model: Default::default(),
            show_reasoning: true,
            model_providers: btree_map!(
                ModelProviderName::from_str("custom_model_provider_name").expect("unexpected err") => ModelProviders::default()
            ),
            log_config: LogConfig::default(),
            dingtalk_config: None,
            heartbeat_config: Default::default(),
        }
    }
}

impl Config {
    pub fn default_workdir() -> PathBuf {
        let user_dirs = directories::UserDirs::new().expect("user home not exist!!!");
        user_dirs.home_dir().join(".fastclaw")
    }
}
