use derive_more::{Deref, Display, From, FromStr, Into};
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    From,
    FromStr,
    Display,
    Eq,
    PartialEq,
    Hash,
    Ord,
    PartialOrd,
    Deref,
    Into,
)]
pub struct Prompt(pub String);

impl From<&str> for Prompt {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<super::Text> for Prompt {
    fn from(value: super::Text) -> Self {
        Self(value.0)
    }
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    From,
    FromStr,
    Display,
    Eq,
    PartialEq,
    Hash,
    Ord,
    PartialOrd,
    Deref,
    Into,
)]
pub struct SystemPrompt(Prompt);

impl From<&str> for SystemPrompt {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<String> for SystemPrompt {
    fn from(value: String) -> Self {
        Self(value.into())
    }
}

impl From<super::Text> for SystemPrompt {
    fn from(value: super::Text) -> Self {
        Self(value.into())
    }
}
