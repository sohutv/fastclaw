use crate::agent::AgentId;
use crate::btree_map;
use crate::config::logger::LogConfig;
use crate::config::{AgentSettings, Config};
use crate::model_provider::{ModelName, ModelProviderName, ModelProviders};
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

    pub fn agent_settings(&self, name: &AgentId) -> Option<&AgentSettings> {
        self.agent_settings.get(name)
    }

    pub fn is_master_session_id(&self, session_id: &str) -> bool {
        #[cfg(feature = "channel_dingtalk_channel")]
        if let Some(cfg) = &self.dingtalk_config {
            if cfg.master_user_id.eq_ignore_ascii_case(session_id) {
                return true;
            }
        }
        false
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_model_provider: Default::default(),
            default_model: Default::default(),
            default_show_reasoning: true,
            agent_settings: btree_map!(),
            model_providers: btree_map!(),
            log_config: LogConfig::default(),
            dingtalk_config: None,
            heartbeat_config: Default::default(),
            websearch: None,
        }
    }
}

impl Config {
    pub fn default_workdir() -> PathBuf {
        let user_dirs = directories::UserDirs::new().expect("user home not exist!!!");
        user_dirs.home_dir().join(".fastclaw")
    }
}
