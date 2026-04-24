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
pub struct Text(pub(super)String);

impl From<&str> for Text {
    fn from(value: &str) -> Self {
        value.to_string().into()
    }
}

impl From<super::Prompt> for Text {
    fn from(value: super::Prompt) -> Self {
        Self(value.0)
    }
}