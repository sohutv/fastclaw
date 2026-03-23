use crate::config::logger::LogConfig;
use crate::model_provider::{ModelName, ModelProviderName, ModelProviders};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod config_;
pub mod logger;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    default_model_provider: ModelProviderName,
    default_model: ModelName,
    pub show_reasoning: bool,
    model_providers: BTreeMap<ModelProviderName, ModelProviders>,
    log_config: LogConfig,
    #[cfg(feature = "channel_dingtalk_channel")]
    pub dingtalk_config: Option<DingTalkConfig>,
    #[serde(default)]
    pub heartbeat_config: HeartbeatConfig,
}

#[cfg(feature = "channel_dingtalk_channel")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingTalkConfig {
    pub credential: dingtalk_stream::Credential,
    pub master_user_id: String,
    pub allow_user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// interval in seconds
    pub interval: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self { interval: 60 }
    }
}
