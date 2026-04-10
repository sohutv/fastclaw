use crate::config::Workspace;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::PathBuf;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenArgs {
    pub prompt: Prompt,
}

impl<P: Into<Prompt>> From<P> for ImageGenArgs {
    fn from(value: P) -> Self {
        Self {
            prompt: value.into(),
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
    //Url(url::Url),
    File { path: PathBuf, format: ImgFormat },
}
#[derive(Debug, Clone, Copy)]
pub enum ImgFormat {
    Jpg,
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
