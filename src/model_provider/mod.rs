use derive_more::{Deref, Display, From, FromStr};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};

#[cfg(feature = "openai_compatible")]
pub mod openai_compatible;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "provider_type")]
pub enum ModelProvider {
    #[cfg(feature = "openai_compatible")]
    OpenaiCompatible(openai_compatible::OpenaiCompatible),
}

impl Default for ModelProvider {
    fn default() -> Self {
        Self::OpenaiCompatible(Default::default())
    }
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    From,
    FromStr,
    Deref,
    Eq,
    PartialEq,
    Hash,
    Ord,
    PartialOrd,
    Default,
    Display,
)]
pub struct ModelProviderName(String);
#[derive(Debug, Clone, Serialize, Deserialize, From, FromStr, Deref, Eq, PartialEq, Default)]
pub struct Model(String);
#[derive(Debug, Clone, Serialize, Deserialize, From, Deref, PartialEq)]
pub struct Temperature(f64);

impl Eq for Temperature {}

impl Default for Temperature {
    fn default() -> Self {
        Self(1.0)
    }
}
