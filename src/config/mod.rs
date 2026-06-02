use crate::agent::{AgentId, AgentSettings};
use crate::config::logger::LogConfig;
use crate::model_provider::{ModelProviderName, ModelProviders};
use crate::ModelName;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod r#type;
use crate::service_provider::{
    EmbeddingConfigs, ImageEnhancerConfigs, ImageGenConfigs, StorageConfigs, WebsearchConfigs,
};
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
    pub dingtalk_config: Option<crate::channels::dingtalk_channel::DingTalkConfig>,
    #[cfg(feature = "channel_wechat_channel")]
    pub wechat_config: Option<crate::channels::wechat_channel::WechatConfig>,
    #[cfg(feature = "channel_http_channel")]
    pub http_config: Option<crate::channels::http_channel::HttpChannelConfig>,
    #[serde(default)]
    pub heartbeat_config: HeartbeatConfig,
    pub websearch: Option<WebsearchConfigs>,
    pub imagegen: Option<ImageGenConfigs>,
    pub image_enhancer: Option<ImageEnhancerConfigs>,
    pub storage: Option<StorageConfigs>,
    pub embedding: Option<EmbeddingConfigs>,
    #[serde(default)]
    pub mcp_tools: Option<McpToolSetConfigs>,
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
use crate::tools::mcp_tool::McpToolSetConfigs;
