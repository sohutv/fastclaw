use derive_more::{Deref, Display, From, FromStr, Into};
use serde::{Deserialize, Serialize};
use std::ops::Add;

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

impl<P: Into<Self>> Add<P> for Prompt {
    type Output = Self;

    fn add(self, rhs: P) -> Self::Output {
        let p = format!(
            r#"{}
        {}"#,
            self,
            rhs.into()
        );
        p.into()
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
pub struct Preamble(Prompt);

impl Add<Self> for Preamble {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        (self.0 + rhs.0).into()
    }
}

impl From<&str> for Preamble {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<String> for Preamble {
    fn from(value: String) -> Self {
        Self(value.into())
    }
}

impl From<super::Text> for Preamble {
    fn from(value: super::Text) -> Self {
        Self(value.into())
    }
}
