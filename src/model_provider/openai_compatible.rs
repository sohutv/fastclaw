use crate::model_provider::{ModelName, ModelProvider, ModelSettings};
use rig::client::Client;
use rig::providers::openai;
use rig::providers::openai::OpenAICompletionsExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenaiCompatible {
    pub api_key: crate::config::ApiKey,
    pub api_url: crate::config::ApiUrl,
    pub models: BTreeMap<ModelName, ModelSettings>,
}

impl ModelProvider for OpenaiCompatible {
    type Client = Client<OpenAICompletionsExt>;

    fn completion_client(&self) -> crate::Result<Client<OpenAICompletionsExt>> {
        let client = openai::CompletionsClient::builder()
            .base_url(&*self.api_url)
            .api_key(&*self.api_key)
            .build()?;
        Ok(client)
    }

    fn model_settings(&self, model: &ModelName) -> Option<&ModelSettings> {
        self.models.get(model)
    }
}
