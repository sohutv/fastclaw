use derive_more::{Deref, Display, From, FromStr};
use rig::client::CompletionClient;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "model_provider_openai_compatible")]
pub mod openai_compatible;

pub trait ModelProvider<Client>: Clone
where
    Client: CompletionClient,
{
    fn completion_client(&self) -> crate::Result<Client>;

    fn model_settings(&self, model: &ModelName) -> Option<&ModelSettings>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider_type")]
pub enum ModelProviders {
    #[cfg(feature = "model_provider_openai_compatible")]
    OpenaiCompatible(openai_compatible::OpenaiCompatible),
}

impl Default for ModelProviders {
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
    Ord,
    PartialOrd,
    Default,
)]
pub struct ModelName(String);
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelSettings {
    pub temperature: Temperature,
    pub vision: bool,
    pub audio: bool,
    pub video: bool,
    pub document: bool,
    pub websearch: bool,
    pub reasoning: bool,
    pub tool: bool,
    pub reranker: bool,
    pub embedding: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, From, Deref, PartialEq)]
pub struct Temperature(f64);

impl Eq for Temperature {}

impl Default for Temperature {
    fn default() -> Self {
        Self(1.0)
    }
}
