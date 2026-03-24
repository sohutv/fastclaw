use crate::model_provider::{ModelName, ModelProvider, ModelSettings};
use derive_more::{Deref, From, FromStr};
use rig::client::Client;
use rig::providers::openai;
use rig::providers::openai::OpenAICompletionsExt;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::str::FromStr;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenaiCompatible {
    pub api_key: ApiKey,
    pub api_url: ApiUrl,
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

#[derive(Debug, Clone, Serialize, Deserialize, FromStr, From, Deref, Eq, PartialEq, Default)]
pub struct ApiKey(String);

#[derive(Debug, Clone, FromStr, From, Deref, Eq, PartialEq)]
pub struct ApiUrl(Url);
impl Default for ApiUrl {
    fn default() -> Self {
        Self(Url::from_str("https://api.openai.com/v1").expect("unexpected url"))
    }
}

impl Serialize for ApiUrl {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let url_string = self.0.to_string();
        serializer.serialize_str(&url_string)
    }
}

impl<'de> Deserialize<'de> for ApiUrl {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let url_string = String::deserialize(deserializer)?;
        let url = Url::from_str(&url_string).map_err(|err| D::Error::custom(err.to_string()))?;
        Ok(Self(url))
    }
}
