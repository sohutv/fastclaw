use crate::agent::{AgentId, AgentSettings};
use crate::channels::dingtalk_channel::DingTalkConfig;
use crate::config::logger::LogConfig;
use crate::model_provider::{ModelName, ModelProviderName, ModelProviders};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod r#type;
use crate::service_provider::WebsearchConfigs;
pub use r#type::*;

mod config_;
pub mod logger;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_model_provider: ModelProviderName,
    pub default_model: ModelName,
    pub default_show_reasoning: bool,
    pub agent_settings: BTreeMap<AgentId, AgentSettings>,
    pub model_providers: BTreeMap<ModelProviderName, ModelProviders>,
    pub log_config: LogConfig,
    #[cfg(feature = "channel_dingtalk_channel")]
    pub dingtalk_config: Option<DingTalkConfig>,
    #[serde(default)]
    pub heartbeat_config: HeartbeatConfig,
    pub websearch: Option<WebsearchConfigs>,
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

mod workspace;
pub use workspace::*;
