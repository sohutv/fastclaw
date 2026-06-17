use crate::channels::SessionId;
use derive_more::{Deref, Display, From};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::ops::Deref;

const AGENT_MAIN: &str = "main";

#[derive(
    Debug, Clone, Deref, Eq, PartialEq, Ord, PartialOrd, Display, Serialize, Deserialize, Hash,
)]
#[serde(default)]
pub struct AgentId(String);

#[derive(
    Debug, Clone, Deref, Eq, PartialEq, Ord, PartialOrd, Display, Serialize, Deserialize, Hash,
)]
pub struct AgentGroup(String);

impl AgentId {
    pub fn main() -> (Self, AgentGroup) {
        (AGENT_MAIN.into(), AgentGroup::main())
    }

    pub fn is_main(&self) -> bool {
        self.deref().eq(AGENT_MAIN)
    }
}

impl<S: Into<String>> From<S> for AgentId {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self::main().0
    }
}

impl AgentGroup {
    pub fn main() -> Self {
        AGENT_MAIN.into()
    }

    pub fn is_main(&self) -> bool {
        self.deref().eq(AGENT_MAIN)
    }

    pub fn ignore_store(&self) -> bool {
        self.is_main()
    }
}

impl<S: Into<String>> From<S> for AgentGroup {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

impl Default for AgentGroup {
    fn default() -> Self {
        Self::main()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, From, Serialize, Deserialize, Hash)]
pub enum OwnerSession {
    GlobalShare,
    Private(SessionId),
}

impl Display for OwnerSession {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            OwnerSession::GlobalShare => write!(f, "GlobalShare"),
            OwnerSession::Private(s) => write!(f, "{}", s),
        }
    }
}
