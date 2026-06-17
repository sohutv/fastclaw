use crate::channels::{SessionConfig, SessionId, SessionSettings, SessionSettingsProvider};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WechatConfig {
    pub session_config: SessionConfig,
}

impl WechatConfig {
    pub(super) fn session_id(&self) -> &SessionId {
        &self.session_config.session_id
    }
}

impl SessionSettingsProvider for WechatConfig {
    fn session_settings(&self, session_id: &SessionId) -> crate::Result<&SessionSettings> {
        if self.session_config.session_id.eq(session_id) {
            Ok(&self.session_config.settings)
        } else {
            Err(anyhow!("session_id {session_id} is forbidden"))
        }
    }
}
