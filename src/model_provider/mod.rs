use crate::ModelName;
use derive_more::{Deref, Display, From, FromStr};
use rig::client::CompletionClient;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
#[cfg(feature = "model_provider_openai_compatible")]
pub mod openai_compatible;

pub trait ModelProvider: Clone {
    type Client: CompletionClient;
    fn completion_client(&self) -> crate::Result<Self::Client>;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSettings {
    pub vision: bool,
    pub audio: bool,
    pub video: bool,
    pub document: bool,
    pub websearch: bool,
    pub reasoning: bool,
    pub tool: bool,
    pub reranker: bool,
    pub embedding: bool,
    pub max_tokens: u64,
    pub reasoning_effort_mapping: ReasoningEffortMapping,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            vision: true,
            audio: false,
            video: false,
            document: false,
            websearch: false,
            reasoning: true,
            tool: true,
            reranker: false,
            embedding: false,
            max_tokens: 65536,
            reasoning_effort_mapping: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Display, Default)]
pub enum ReasoningEffort {
    #[display("minimal")]
    Minimal,
    #[default]
    #[display("low")]
    Low,
    #[display("medium")]
    Medium,
    #[display("high")]
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningEffortMapping {
    /// Zero-thought, direct answer.
    minimal: ReasoningEffortVal,
    /// Lightweight, optimized for speed.
    low: ReasoningEffortVal,
    /// Balanced, optimizing for both speed and depth.
    medium: ReasoningEffortVal,
    ///  Deep analysis, for complex tasks.
    high: ReasoningEffortVal,
}

impl Default for ReasoningEffortMapping {
    fn default() -> Self {
        Self {
            minimal: ReasoningEffortVal(ReasoningEffort::Minimal.to_string()),
            low: ReasoningEffortVal(ReasoningEffort::Low.to_string()),
            medium: ReasoningEffortVal(ReasoningEffort::Medium.to_string()),
            high: ReasoningEffortVal(ReasoningEffort::High.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, From, FromStr, Deref)]
pub struct ReasoningEffortVal(String);

impl ReasoningEffortMapping {
    pub fn from(&self, effort: ReasoningEffort) -> &ReasoningEffortVal {
        match effort {
            ReasoningEffort::Minimal => &self.minimal,
            ReasoningEffort::Low => &self.low,
            ReasoningEffort::Medium => &self.medium,
            ReasoningEffort::High => &self.high,
        }
    }
}
