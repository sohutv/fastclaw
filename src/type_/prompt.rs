use derive_more::{Deref, Display, From, FromStr, Into};
use serde::{Deserialize, Serialize};
use std::ops::Deref;

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
    Default,
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

impl Prompt {
    pub fn append_line<P: Into<Prompt>>(&self, prompt: P) -> Prompt {
        let p = format!(
            r#"{}
        {}"#,
            self,
            prompt.into()
        );
        p.into()
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
    Default,
)]
pub struct SystemPrompt(Prompt);

impl SystemPrompt {
    pub fn append_line<P: Into<SystemPrompt>>(&self, prompt: P) -> SystemPrompt {
        self.deref().append_line(prompt.into().0).into()
    }
}

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
