use crate::channels::{SessionConfig, SessionId, SessionSettings, SessionSettingsProvider};
use anyhow::anyhow;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::ops::Deref;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpChannelConfig {
    pub addr: String,
    pub session_configs: Vec<SessionConfig>,
}

impl HttpChannelConfig {
    pub(super) fn master_session_ids(&self) -> Vec<&SessionId> {
        self.session_configs
            .iter()
            .map(|it| &it.session_id)
            .flat_map(|it| {
                if let SessionId::Master { .. } = it {
                    Some(it)
                } else {
                    None
                }
            })
            .collect_vec()
    }
}

impl SessionSettingsProvider for HttpChannelConfig {
    fn session_settings(&self, session_id: &SessionId) -> crate::Result<&SessionSettings> {
        let dst = self
            .session_configs
            .iter()
            .find(|it| it.session_id.eq(session_id))
            .ok_or(anyhow!("session_id {session_id} is forbidden",))?;
        Ok(&dst.settings)
    }
}

impl<S: AsRef<str>> TryFrom<(S, &HttpChannelConfig)> for SessionId {
    type Error = anyhow::Error;

    fn try_from((raw_session_id, config): (S, &HttpChannelConfig)) -> Result<Self, Self::Error> {
        let raw_session_id = raw_session_id.as_ref();
        let dst = config
            .session_configs
            .iter()
            .map(|it| &it.session_id)
            .find(|&it| it.deref().eq(raw_session_id))
            .ok_or(anyhow!("session_id {raw_session_id} is forbidden",))?;
        Ok(dst.clone())
    }
}
