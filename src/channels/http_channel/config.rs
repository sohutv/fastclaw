use crate::channels::{SessionId, SessionSettings, SessionSettingsProvider};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Deref;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpChannelConfig {
    pub addr: String,
    pub allow_session_ids: HashMap<SessionId, SessionSettings>,
}

impl SessionSettingsProvider for HttpChannelConfig {
    fn session_settings(&self, session_id: &SessionId) -> crate::Result<&SessionSettings> {
        let dst = self
            .allow_session_ids
            .get(session_id)
            .ok_or(anyhow!("session_id {session_id} is forbidden",))?;
        Ok(dst)
    }
}

impl<S: AsRef<str>> TryFrom<(S, &HttpChannelConfig)> for SessionId {
    type Error = anyhow::Error;

    fn try_from((raw_session_id, config): (S, &HttpChannelConfig)) -> Result<Self, Self::Error> {
        let raw_session_id = raw_session_id.as_ref();
        let dst = config
            .allow_session_ids
            .keys()
            .find(|&it| it.deref().eq(raw_session_id))
            .ok_or(anyhow!("session_id {raw_session_id} is forbidden",))?;
        Ok(dst.clone())
    }
}
