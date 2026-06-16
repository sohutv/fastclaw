use std::collections::HashMap;
use crate::channels::{SessionId, SessionSettings, SessionSettingsProvider};
use anyhow::anyhow;
use itertools::Itertools;
use std::ops::Deref;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingTalkConfig {
    pub credential: dingtalk_stream::Credential,
    pub allow_session_ids: HashMap<SessionId, SessionSettings>,
}

impl DingTalkConfig {
    pub(super) fn master_session_ids(&self) -> Vec<&SessionId> {
        self.allow_session_ids
            .keys()
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

impl SessionSettingsProvider for DingTalkConfig {
    fn session_settings(&self, session_id: &SessionId) -> crate::Result<&SessionSettings> {
        let dst = self
            .allow_session_ids
            .get(session_id)
            .ok_or(anyhow!("session_id {session_id} is forbidden",))?;
        Ok(dst)
    }
}

impl<S: AsRef<str>> TryFrom<(S, &DingTalkConfig)> for SessionId {
    type Error = anyhow::Error;

    fn try_from((raw_session_id, config): (S, &DingTalkConfig)) -> Result<Self, Self::Error> {
        let raw_session_id = raw_session_id.as_ref();
        let dst = config
            .allow_session_ids
            .keys()
            .find(|&it| it.deref().eq(raw_session_id))
            .ok_or(anyhow!("session_id {raw_session_id} is forbidden",))?;
        Ok(dst.clone())
    }
}
