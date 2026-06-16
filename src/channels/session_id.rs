use derive_more::{Deref, Display, From};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::ops::Deref;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash, Display, From)]
pub enum SessionId {
    Master(Master),
    Anonymous(Anonymous),
    Group(Group),
}

impl Deref for SessionId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        let val: &str = match self {
            SessionId::Master(val) => val,
            SessionId::Anonymous(val) => val,
            SessionId::Group(val) => val,
        };
        val
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionSettings {
    pub show_connected: bool,
    pub show_start: bool,
    pub show_toolcall: bool,
    pub show_reasoning: bool,
    pub show_notify: bool,
    pub show_compacting: bool,
    pub show_compacting_ok: bool,
    pub show_compacting_err: bool,
    pub show_compacting_ignore: bool,
    pub show_error: bool,
    pub show_disconnected: bool,
    pub show_token_usage: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            show_connected: false,
            show_start: true,
            show_toolcall: false,
            show_reasoning: false,
            show_notify: false,
            show_compacting: false,
            show_compacting_ok: false,
            show_compacting_err: true,
            show_compacting_ignore: false,
            show_error: true,
            show_disconnected: false,
            show_token_usage: true,
        }
    }
}

pub trait SessionSettingsProvider {
    fn session_settings(&self, session_id: &SessionId) -> crate::Result<&SessionSettings>;
}

impl SessionId {
    pub fn settings<'a, P: SessionSettingsProvider>(
        &self,
        provider: &'a P,
    ) -> crate::Result<&'a SessionSettings> {
        provider.session_settings(self)
    }
}

#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize, Display, Deref,
)]
pub struct Master(pub String);

impl<S> From<S> for Master
where
    S: AsRef<str>,
{
    fn from(val: S) -> Self {
        Master(val.as_ref().to_string())
    }
}

#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize, Display, Deref,
)]
pub struct Anonymous(pub String);

impl<S> From<S> for Anonymous
where
    S: AsRef<str>,
{
    fn from(val: S) -> Self {
        Anonymous(val.as_ref().to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Display, Deref)]
#[display("{name:?}[{id}]:{user_id}")]
pub struct Group {
    pub id: String,
    #[deref]
    pub user_id: GroupUserId,
    pub name: Option<String>,
}
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize, Display, Deref,
)]
pub enum GroupUserId {
    Master(Anonymous),
    Anonymous(Anonymous),
}

impl Eq for Group {}

impl PartialEq for Group {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl Hash for Group {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> From<&T> for SessionId
where
    T: Into<SessionId> + Clone,
{
    fn from(value: &T) -> Self {
        value.clone().into()
    }
}
