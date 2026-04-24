use base64::Engine;
use derive_more::{Deref, From, FromStr};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::str::FromStr;

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

#[derive(Debug, Clone, Serialize, Deserialize, From, Deref, FromStr)]
pub struct Base64(String);

impl Base64 {
    pub fn data(&self) -> String {
        lazy_static::lazy_static! {
            static ref HEADER_REGEX: regex::Regex = regex::Regex::from_str(r#"^data:image/(\w+);base64,"#).unwrap();
        };
        let str = self.deref();
        let data = HEADER_REGEX.replace(str, "");
        data.to_string()
    }
}

impl TryFrom<Base64> for Vec<u8> {
    type Error = anyhow::Error;

    fn try_from(value: Base64) -> Result<Self, Self::Error> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(value.deref())?;
        Ok(bytes)
    }
}

impl TryFrom<&[u8]> for Base64 {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(value);
        Ok(b64.into())
    }
}

mod media;
#[allow(unused)]
pub use media::*;

mod text;
pub use text::*;
mod image;
pub use image::*;

mod prompt;
pub use prompt::*;
