use anyhow::anyhow;
use base64::Engine;
use image::{DynamicImage, EncodableLayout};
use mime::Mime;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use derive_more::Deref;

lazy_static::lazy_static! {
    static ref REGEX_0: regex::Regex = regex::Regex::from_str(r#"^data:((\w+)/(\w+));base64,(.+)$"#).unwrap();
}

#[derive(Debug, Clone,Deref)]
pub struct Base64Res {
    pub mime: Mime,
    #[deref]
    pub content: Vec<u8>,
}

impl<'de> Deserialize<'de> for Base64Res {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = String::deserialize(deserializer)?;
        if let Some((_, [mime, _type, _sub_type, content])) =
            REGEX_0.captures(&data).map(|it| it.extract())
        {
            Ok(Base64Res {
                mime: mime
                    .parse()
                    .map_err(|err| D::Error::custom(format!("{err}")))?,
                content: base64::engine::general_purpose::STANDARD
                    .decode(content)
                    .map_err(|err| D::Error::custom(format!("{err}")))?,
            })
        } else {
            Err(D::Error::custom(format!(
                "deserialize failed, data: {data}"
            )))
        }
    }
}

impl Serialize for Base64Res {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let data = self.to_string();
        data.serialize(serializer)
    }
}

impl Display for Base64Res {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let data = format!(
            "data:{};base64,{}",
            self.mime,
            base64::engine::general_purpose::STANDARD.encode(&self.content)
        );
        write!(f, "{}", data)
    }
}

impl TryFrom<&Base64Res> for DynamicImage {
    type Error = anyhow::Error;

    fn try_from(Base64Res { mime, content }: &Base64Res) -> Result<Self, Self::Error> {
        let format = image::ImageFormat::from_mime_type(mime).ok_or(anyhow!("not image"))?;
        let image = image::load_from_memory_with_format(content.as_bytes(), format)?;
        Ok(image)
    }
}
