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
pub struct Prompt(pub(super) String);

impl From<&str> for Prompt {
    fn from(value: &str) -> Self {
        value.to_string().into()
    }
}

impl From<super::Text> for Prompt {
    fn from(value: super::Text) -> Self {
        Self(value.0)
    }
}
