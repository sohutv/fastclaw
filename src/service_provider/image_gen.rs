use crate::config::Workspace;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::PathBuf;

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
    File(PathBuf),
}

pub trait ImageGen: Sync + Send {
    async fn generate(
        &self,
        workspace: &'static Workspace,
        args: ImageGenArgs,
    ) -> crate::Result<ImageGenResult>;
}

pub trait ImageGenConfig: Sync + Send {
    type T: ImageGen;
    async fn try_into_image_gen(&self) -> crate::Result<Self::T>;
}
