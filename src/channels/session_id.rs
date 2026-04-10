use derive_more::{Deref, Display};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Deref;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionId {
    Master {
        val: Master,
        settings: SessionSettings,
    },
    Anonymous {
        val: Anonymous,
        settings: SessionSettings,
    },
    Group {
        val: Group,
        settings: SessionSettings,
    },
}

impl Display for SessionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            SessionId::Master { val, .. } => val.as_str(),
            SessionId::Anonymous { val, .. } => val.as_str(),
            SessionId::Group { val, .. } => val.as_str(),
        };
        write!(f, "{}", str)
    }
}

impl SessionId {
    pub fn settings(&self) -> &SessionSettings {
        match self {
            SessionId::Master { settings, .. } => settings,
            SessionId::Anonymous { settings, .. } => settings,
            SessionId::Group { settings, .. } => settings,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionSettings {
    pub show_start: bool,
    pub show_toolcall: bool,
    pub show_reasoning: bool,
    pub show_notify: bool,
    pub show_compacting: bool,
    pub show_compacting_ok: bool,
    pub show_compacting_err: bool,
    pub show_compacting_ignore: bool,
    pub show_error: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            show_start: true,
            show_toolcall: false,
            show_reasoning: false,
            show_notify: false,
            show_compacting: true,
            show_compacting_ok: true,
            show_compacting_err: true,
            show_compacting_ignore: false,
            show_error: true,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Display, Deref)]
pub struct Master(pub String);
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Display, Deref)]
pub struct Anonymous(pub String);
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Display)]
pub enum UserId {
    Master(Master),
    Anonymous(Anonymous),
}

impl Deref for UserId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match self {
            UserId::Master(val) => val,
            UserId::Anonymous(val) => val,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Display, Deref)]
#[display("{name:?}[{id}]:{user_id}")]
pub struct Group {
    pub id: String,
    #[deref]
    pub session_id: String,
    pub user_id: UserId,
    pub name: Option<String>,
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

impl Deref for SessionId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        let val: &str = match self {
            SessionId::Master { val, .. } => val,
            SessionId::Anonymous { val, .. } => val,
            SessionId::Group { val, .. } => val,
        };
        val
    }
}

impl UserId {
    pub fn master<S: AsRef<str>>(val: S) -> UserId {
        UserId::Master(val.into())
    }

    pub fn anonymous<S: AsRef<str>>(val: S) -> UserId {
        UserId::Anonymous(val.into())
    }
}

impl<S> From<S> for Master
where
    S: AsRef<str>,
{
    fn from(val: S) -> Self {
        Master(val.as_ref().to_string())
    }
}

impl<S> From<S> for Anonymous
where
    S: AsRef<str>,
{
    fn from(val: S) -> Self {
        Anonymous(val.as_ref().to_string())
    }
}

impl Into<UserId> for &SessionId {
    fn into(self) -> UserId {
        match self {
            SessionId::Master { val, .. } => UserId::Master(val.clone()),
            SessionId::Anonymous { val, .. } => UserId::Anonymous(val.clone()),
            SessionId::Group { val, .. } => val.user_id.clone(),
        }
    }
}

impl From<Master> for UserId {
    fn from(value: Master) -> Self {
        UserId::Master(value)
    }
}

impl From<&Master> for UserId {
    fn from(value: &Master) -> Self {
        UserId::Master(value.clone())
    }
}

impl From<Anonymous> for UserId {
    fn from(value: Anonymous) -> Self {
        UserId::Anonymous(value)
    }
}

impl From<&Anonymous> for UserId {
    fn from(value: &Anonymous) -> Self {
        UserId::Anonymous(value.clone())
    }
}
