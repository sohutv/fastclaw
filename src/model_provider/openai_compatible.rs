use crate::model_provider::{Model, ModelProvider, Temperature};
use derive_more::{Deref, From, FromStr};
use rig::client::Client;
use rig::providers::openai;
use rig::providers::openai::OpenAICompletionsExt;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
pub struct OpenaiCompatible {
    pub api_key: ApiKey,
    pub api_url: ApiUrl,
    pub model: Vec<Model>,
    pub temperature: Temperature,
}

impl OpenaiCompatible {
    pub fn completion_client(&self) -> crate::Result<Client<OpenAICompletionsExt>> {
        let client = openai::CompletionsClient::builder()
            .base_url(&*self.api_url)
            .api_key(&*self.api_key)
            .build()?;
        Ok(client)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_provider::openai_compatible::{ApiKey, ApiUrl};
    use anyhow::anyhow;
    use std::str::FromStr;

    #[test]
    fn test_to_toml() -> crate::Result<()> {
        let config = ModelProvider::OpenaiCompatible(OpenaiCompatible {
            api_key: ApiKey::from_str("sk_123")?,
            api_url: ApiUrl::from_str("https://api.openai.com/v1")?,
            model: vec![Model::from_str("gemini-3-flash-preview")?],
            temperature: Temperature::default(),
        });
        let config_toml = toml::to_string(&config)?;
        println!("{}", config_toml);
        Ok(())
    }

    #[test]
    fn test_from_toml() -> crate::Result<()> {
        let toml = r#"
provider_type = "OpenaiCompatible"
api_key = "sk_123"
api_url = "https://api.openai.com/v1"
model = ["gemini-3-flash-preview"]
temperature = 1.0
        "#;
        let Ok(ModelProvider::OpenaiCompatible(config)) = toml::from_str::<ModelProvider>(&toml)
        else {
            return Err(anyhow!("Failed to parse config from TOML"));
        };
        println!("{config:?}");
        let OpenaiCompatible {
            api_key,
            api_url,
            model,
            temperature,
            ..
        } = config;
        assert_eq!(api_key, ApiKey::from_str("sk_123")?);
        assert_eq!(api_url, ApiUrl::from_str("https://api.openai.com/v1")?);
        assert_eq!(model, vec![Model::from_str("gemini-3-flash-preview")?]);
        assert_eq!(temperature, Temperature::default());
        Ok(())
    }
}
