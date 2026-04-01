use crate::channels::Session;
use derive_more::{Deref, Display};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::ops::Deref;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Display)]
pub enum SessionId {
    Master(Master),
    Anonymous(Anonymous),
    Group(Group),
}

#[cfg(test)]
mod tests {
    use crate::btree_map;
    use crate::channels::{Group, Master, SessionId, UserId};
    use std::collections::BTreeMap;

    #[test]
    fn test() {
        let map: BTreeMap<String, SessionId> = btree_map!(
            "group:cidUBGm9d3LTYczXMWuDIaXxg==:032615015535634423".to_string() =>  SessionId::Group(Group{
                id: "cidUBGm9d3LTYczXMWuDIaXxg==".to_string(),
                session_id:"group:cidUBGm9d3LTYczXMWuDIaXxg==:032615015535634423".to_string(),
                user_id: UserId::Master(Master("032615015535634423".to_string())),
                name: Some("GGGNNN".to_string())
            })
        );
        let str = toml::to_string(&map).unwrap();
        print!("{str}")
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
            SessionId::Master(val) => val,
            SessionId::Anonymous(val) => val,
            SessionId::Group(val) => val,
        };
        val
    }
}

impl<U> From<U> for SessionId
where
    U: Into<UserId>,
{
    fn from(value: U) -> Self {
        let user_id = value.into();
        match user_id {
            UserId::Master(id) => SessionId::Master(id),
            UserId::Anonymous(id) => SessionId::Anonymous(id),
        }
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
            SessionId::Master(val) => UserId::Master(val.clone()),
            SessionId::Anonymous(val) => UserId::Anonymous(val.clone()),
            SessionId::Group(Group { user_id, .. }) => user_id.clone(),
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

impl From<&SessionId> for Session {
    fn from(value: &SessionId) -> Self {
        match value {
            SessionId::Master(user_id) => Session::Private {
                session_id: user_id.into(),
            },
            SessionId::Anonymous(user_id) => Session::Private {
                session_id: user_id.into(),
            },
            SessionId::Group(group) => Session::Group {
                session_id: group.clone(),
                group_name: group.name.clone(),
            },
        }
    }
}
