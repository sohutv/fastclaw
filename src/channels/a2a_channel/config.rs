use crate::channels::{SessionId, SessionSettings, SessionSettingsProvider};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AChannelConfig {
    #[serde(default)]
    pub session_settings: SessionSettings,
}

impl SessionSettingsProvider for A2AChannelConfig {
    fn session_settings(&self, _: &SessionId) -> crate::Result<&SessionSettings> {
        Ok(&self.session_settings)
    }
}
