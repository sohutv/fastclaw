use super::super::imagegen::*;
use crate::ModelName;
use crate::config::{ApiKey, ApiUrl, Workspace};
use crate::service_provider::ImgFormat::Png;
use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolcengineImageGenConfig {
    api_url: ApiUrl,
    api_key: ApiKey,
    model: ModelName,
}

#[derive(Clone)]
pub struct VolcengineImageGen {
    config: VolcengineImageGenConfig,
}

#[async_trait]
impl ImageGenConfig for VolcengineImageGenConfig {
    type T = VolcengineImageGen;

    async fn try_into_imagegen(&self) -> crate::Result<Self::T> {
        Ok(VolcengineImageGen {
            config: self.clone(),
        })
    }
}

#[async_trait]
impl ImageGen for VolcengineImageGen {
    async fn generate(
        &self,
        workspace: &'static Workspace,
        ImageGenArgs { prompt, images }: ImageGenArgs,
    ) -> crate::Result<ImageGenResult> {
        let images = {
            let images = if let Some(images) = images {
                images.as_base64().await?
            } else {
                vec![]
            };
            match images.len() {
                0 => serde_json::Value::Null,
                1 => {
                    let Some(image) = images.first().map(|it| &**it) else {
                        unreachable!()
                    };
                    serde_json::Value::String(image.clone())
                }
                _ => serde_json::to_value(&images[..14])?,
            }
        };

        let response = reqwest::Client::default()
            .post(self.config.api_url.as_str())
            .header(
                "Authorization",
                format!("Bearer {}", self.config.api_key.as_str()),
            )
            .header("Content-Type", "application/json")
            .json(&json!({
                "model": self.config.model.as_str(),
                "prompt": &prompt,
                "image": &images,
                "size": "2K",
                "output_format": "png",
                "watermark": false,
            }))
            .send()
            .await
            .map_err(|err| anyhow!(err))?
            .json::<Response>()
            .await
            .map_err(|err| anyhow!(err))?;
        let result = response.try_into_image_gen_result(workspace).await?;
        Ok(result)
    }
}

#[derive(Serialize, Deserialize)]
struct Response {
    model: String,
    created: u64,
    data: Vec<Data>,
    usage: Usage,
}
#[derive(Serialize, Deserialize)]
struct Data {
    url: String,
    size: String,
}

#[derive(Serialize, Deserialize)]
struct Usage {
    generated_images: usize,
    output_tokens: u64,
    total_tokens: u64,
}

impl Response {
    async fn try_into_image_gen_result(
        &self,
        workspace: &'static Workspace,
    ) -> crate::Result<ImageGenResult> {
        let mut images = Vec::with_capacity(self.data.len());
        for Data { url, .. } in &self.data {
            let bytes = reqwest::Client::default()
                .get(url)
                .send()
                .await?
                .bytes()
                .await?;
            let filepath = workspace
                .downloads_path()
                .join(format!("{}.png", uuid::Uuid::new_v4()));
            let _ = tokio::fs::write(&filepath, bytes).await?;
            let _ = images.push(Image::File {
                path: filepath,
                format: Png,
            });
        }
        Ok(ImageGenResult { images })
    }
}

impl VolcengineImageGenConfig {
    #[allow(unused)]
    fn from_env() -> crate::Result<Self> {
        Ok(Self {
            api_url: ApiUrl::from_str(std::env::var("VOLCENGINE_IMAGE_GEN_API_URL")?.as_str())?,
            api_key: std::env::var("VOLCENGINE_IMAGE_GEN_API_KEY")?.into(),
            model: std::env::var("VOLCENGINE_IMAGE_GEN_MODEL_NAME")?.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Workspace;
    use crate::service_provider::volcengine::imagegen::VolcengineImageGenConfig;
    use crate::service_provider::{Image, ImageGen, ImageGenConfig, ImageGenResult};

    #[tokio::test]
    async fn test_image_gen() -> crate::Result<()> {
        let config = VolcengineImageGenConfig::from_env()?;
        let websearch = config.try_into_imagegen().await?;
        let workspace: &'static Workspace = Box::leak(Box::new(Workspace::init("/tmp").await?));
        let ImageGenResult { images, .. } = websearch
            .generate(workspace, "一只阿拉蕾风格的兔子".into())
            .await?;
        for image in images {
            match image {
                Image::File { path: filepath, .. } => {
                    println!("image-file: {}", filepath.display())
                }
                _ => {}
            }
        }
        Ok(())
    }
}
