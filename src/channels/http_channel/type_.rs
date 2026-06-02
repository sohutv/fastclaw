use crate::channels::SessionId;
use crate::type_::Base64Res;
use anyhow::anyhow;
use derive_more::{Deref, Display};
use image::{DynamicImage, ImageFormat};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpReqMessage {
    #[serde(default)]
    pub message_id: MessageId,
    pub payloads: Vec<Payload>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpRespMessage {
    pub message_id: MessageId,
    pub user_id: UserId,
    pub payloads: Vec<Payload>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Display, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[display("{_0}")]
pub struct MessageId(String);

impl Default for MessageId {
    fn default() -> Self {
        Uuid::new_v4().into()
    }
}

impl From<Uuid> for MessageId {
    fn from(value: Uuid) -> Self {
        Self(value.to_string())
    }
}

#[derive(
    Debug, Clone, Deserialize, Serialize, Display, Eq, PartialEq, Ord, PartialOrd, Hash, Deref,
)]
#[display("{_0}")]
pub struct UserId(String);

impl From<&SessionId> for UserId {
    fn from(value: &SessionId) -> Self {
        UserId(value.to_string())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Payload {
    #[serde(rename = "text")]
    Text(String),
    #[serde(rename = "json")]
    Json(serde_json::Value),
    #[serde(rename = "image")]
    Image(Base64Image),
}

impl From<String> for Payload {
    fn from(value: String) -> Self {
        Payload::Text(value)
    }
}

#[derive(Debug, Clone, Deref, Serialize, Deserialize)]
pub struct Base64Image(Base64Res);

impl Base64Image {
    pub fn format(&self) -> crate::Result<ImageFormat> {
        ImageFormat::from_mime_type(&self.mime)
            .ok_or(anyhow!(format!("unexpected mime: {}", self.mime)))
    }

    pub fn extension(&self) -> crate::Result<&'static str> {
        let format = self.format()?;
        let extension = *format
            .extensions_str()
            .first()
            .ok_or(anyhow!(format!("unexpected format")))?;
        Ok(extension)
    }
}

impl TryFrom<&Base64Image> for DynamicImage {
    type Error = anyhow::Error;

    fn try_from(value: &Base64Image) -> Result<Self, Self::Error> {
        DynamicImage::try_from(value.deref())
    }
}
