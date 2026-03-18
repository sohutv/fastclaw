use crate::btree_map;
use crate::model_provider::{Model, ModelProvider, ModelProviderName};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    default_model_provider: ModelProviderName,
    default_model: Model,
    model_providers: BTreeMap<ModelProviderName, ModelProvider>,
}

impl Config {
    pub fn model_provider(&self, name: &ModelProviderName) -> crate::Result<ModelProvider> {
        if let Some(provider) = self.model_providers.get(name).map(|it| it.clone()) {
            Ok(provider)
        } else {
            Err(anyhow!("Model provider not found for name: {}", name))
        }
    }

    pub fn default_model_provider(&self) -> crate::Result<ModelProvider> {
        self.model_provider(&self.default_model_provider)
    }

    pub fn default_model(&self) -> &Model {
        &self.default_model
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_model_provider: Default::default(),
            default_model: Default::default(),
            model_providers: btree_map!(
                ModelProviderName::from_str("custom_model_provider_name").expect("unexpected err") => ModelProvider::default()
            ),
        }
    }
}
