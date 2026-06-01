use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use crate::channels::SessionId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpChannelConfig {
    pub addr: String,
    pub allow_session_ids: BTreeMap<String, SessionId>,
}

impl HttpChannelConfig {
    pub(super) fn allow_session_id<UserId: AsRef<str>>(&self, user_id: UserId) -> Option<&SessionId> {
        self.allow_session_ids.get(user_id.as_ref())
    }
}
