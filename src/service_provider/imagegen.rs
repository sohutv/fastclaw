use crate::config::Workspace;
use crate::type_::{Image, Images, Prompt};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
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

impl<P: Into<Prompt>> From<P> for ImageGenArgs {
    fn from(value: P) -> Self {
        Self {
            prompt: value.into(),
            images: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageGenResult {
    pub images: Vec<Image>,
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
