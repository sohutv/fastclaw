use crate::channels::{SessionId, SessionSettings, SessionSettingsProvider};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WechatConfig {
    pub session_id: SessionId,
    pub session_settings: SessionSettings,
}

impl SessionSettingsProvider for WechatConfig {
    fn session_settings(&self, session_id: &SessionId) -> crate::Result<&SessionSettings> {
        if self.session_id.eq(session_id) {
            Ok(&self.session_settings)
        } else {
            Err(anyhow!("session_id {session_id} is forbidden"))
        }
    }
}
