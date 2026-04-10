use derive_more::{Deref, From, FromStr};
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;
use url::Url;

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
