use crate::config::Workspace;
use async_trait::async_trait;
use base64::Engine;
use derive_more::{Deref, Display, From};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::io::Cursor;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ImageGenConfigs {
    #[cfg(feature = "volcengine")]
    #[serde(rename = "volcengine")]
    Volcengine(super::volcengine::imagegen::VolcengineImageGenConfig),
}

impl ImageGenConfigs {
    pub async fn try_into_imagegen(&self) -> crate::Result<Arc<dyn ImageGen>> {
        match self {
            #[cfg(feature = "volcengine")]
            ImageGenConfigs::Volcengine(config) => {
                let imagegen = config.try_into_imagegen().await?;
                Ok(Arc::new(imagegen))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageGenArgs {
    pub prompt: Prompt,
    pub images: Option<Images>,
}
#[derive(Debug, Clone, Deref, From)]
pub struct Images(Vec<Image>);

impl Images {
    pub async fn as_base64(&self) -> crate::Result<Vec<Base64>> {
        let mut vec = Vec::with_capacity(self.len());
        for image in self.deref() {
            vec.push(image.as_base64().await?);
        }
        Ok(vec)
    }

    pub async fn try_from<S: AsRef<str>>(value: &[S]) -> crate::Result<Self> {
        let mut vec = Vec::with_capacity(value.len());
        for s in value {
            vec.push(Image::try_from(s).await?);
        }
        Ok(vec.into())
    }
}

impl<P: Into<Prompt>> From<P> for ImageGenArgs {
    fn from(value: P) -> Self {
        Self {
            prompt: value.into(),
            images: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt(String);

impl<S: Display> From<S> for Prompt {
    fn from(value: S) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct ImageGenResult {
    pub images: Vec<Image>,
}

#[derive(Debug, Clone)]
pub enum Image {
    #[allow(unused)]
    Url {
        url: url::Url,
        format: ImgFormat,
    },
    File {
        path: PathBuf,
        format: ImgFormat,
    },
    Raw {
        bytes: Vec<u8>,
        format: ImgFormat,
    },
}

impl Image {
    async fn as_base64(&self) -> crate::Result<Base64> {
        match self {
            Image::Url { url, format } => {
                let bytes = reqwest::get(url.as_str()).await?.bytes().await?;
                let string = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Ok(format!("data:image/{};base64,{}", format, string).into())
            }
            Image::File { path, format } => {
                let bytes = tokio::fs::read(path).await?;
                let string = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Ok(format!("data:image/{};base64,{}", format, string).into())
            }
            Image::Raw { bytes, format } => {
                let string = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Ok(format!("data:image/{};base64,{}", format, string).into())
            }
        }
    }

    async fn try_from<S: AsRef<str>>(value: S) -> crate::Result<Self> {
        let str = value.as_ref();
        let bytes = match url::Url::from_str(str) {
            Ok(url) => reqwest::get(url.as_str()).await?.bytes().await?.to_vec(),
            Err(_) => tokio::fs::read(str).await?,
        };
        let image = image::load_from_memory(&bytes)?;
        let mut buf = vec![];
        let mut cursor = Cursor::new(&mut buf);
        let _ = image.write_to(&mut cursor, image::ImageFormat::Png)?;
        Ok(Image::Raw {
            bytes: buf,
            format: ImgFormat::Png,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, From, Deref)]
pub struct Base64(String);

#[derive(Debug, Clone, Copy, Display)]
pub enum ImgFormat {
    #[display("jpg")]
    Jpg,
    #[display("png")]
    Png,
}

impl From<ImgFormat> for mime::Mime {
    fn from(value: ImgFormat) -> Self {
        match value {
            ImgFormat::Jpg => mime::IMAGE_JPEG,
            ImgFormat::Png => mime::IMAGE_PNG,
        }
    }
}

#[async_trait]
pub trait ImageGen: Sync + Send {
    async fn generate(
        &self,
        workspace: &'static Workspace,
        args: ImageGenArgs,
    ) -> crate::Result<ImageGenResult>;
}

#[async_trait]
pub trait ImageGenConfig: Sync + Send {
    type T: ImageGen;
    async fn try_into_imagegen(&self) -> crate::Result<Self::T>;
}
