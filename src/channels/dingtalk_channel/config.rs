use crate::channels::SessionId;
use crate::channels::dingtalk_channel::DingTalkConfig;
use anyhow::anyhow;
use itertools::Itertools;

impl DingTalkConfig {
    fn allow_session_id<UserId: AsRef<str>>(&self, user_id: UserId) -> Option<&SessionId> {
        self.allow_session_ids.get(user_id.as_ref())
    }

    pub(super) fn master_session_ids(&self) -> Vec<&SessionId> {
        self.allow_session_ids
            .values()
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

impl<S: AsRef<str>> TryFrom<(S, &DingTalkConfig)> for SessionId {
    type Error = anyhow::Error;

    fn try_from((session_id_key, config): (S, &DingTalkConfig)) -> Result<Self, Self::Error> {
        match config.allow_session_id(session_id_key.as_ref()) {
            Some(dst) => Ok(dst.clone()),
            None => Err(anyhow!(
                "session_id {} not allowed",
                session_id_key.as_ref()
            )),
        }
    }
}
