use crate::config::Workspace;
use crate::type_::Image;
use async_trait::async_trait;
use derive_more::From;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ImageEnhancerConfigs {
    #[cfg(feature = "volcengine")]
    #[serde(rename = "volcengine")]
    Volcengine(super::volcengine::image_enhancer::VolcengineImageEnhancerConfig),
}

impl ImageEnhancerConfigs {
    pub async fn try_into_image_enhancer(&self) -> crate::Result<Arc<dyn ImageEnhancer>> {
        match self {
            #[cfg(feature = "volcengine")]
            ImageEnhancerConfigs::Volcengine(config) => {
                let enhancer = config.try_into_image_enhancer().await?;
                Ok(Arc::new(enhancer))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageEnhancerArgs {
    pub image: Image,
    pub hdr: Option<Hdr>,
    pub wb: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, From)]
pub struct Hdr {
    strength: f32,
}

impl Hdr {
    pub fn strength(&self) -> f32 {
        match self.strength {
            val @ 0f32..=1f32 => val,
            _ => 1.,
        }
    }
}

impl From<Image> for ImageEnhancerArgs {
    fn from(image: Image) -> Self {
        Self {
            image,
            hdr: None,
            wb: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageEnhancerResult {
    pub image: Image,
}

#[async_trait]
pub trait ImageEnhancer: Sync + Send {
    async fn enhance(
        &self,
        workspace: &'static Workspace,
        args: ImageEnhancerArgs,
    ) -> crate::Result<ImageEnhancerResult>;
}

#[async_trait]
pub trait ImageEnhancerConfig: Sync + Send {
    type T: ImageEnhancer;
    async fn try_into_image_enhancer(&self) -> crate::Result<Self::T>;
}
