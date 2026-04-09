use super::super::image_gen::*;
use crate::ModelName;
use crate::config::{ApiKey, ApiUrl, Workspace};
use anyhow::anyhow;
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

impl ImageGenConfig for VolcengineImageGenConfig {
    type T = VolcengineImageGen;

    async fn try_into_image_gen(&self) -> crate::Result<Self::T> {
        Ok(VolcengineImageGen {
            config: self.clone(),
        })
    }
}

impl ImageGen for VolcengineImageGen {
    async fn generate(
        &self,
        workspace: &'static Workspace,
        ImageGenArgs { prompt }: ImageGenArgs,
    ) -> crate::Result<ImageGenResult> {
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
            let _ = images.push(Image::File(filepath));
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
    use crate::service_provider::volcengine::image_gen::VolcengineImageGenConfig;
    use crate::service_provider::{Image, ImageGen, ImageGenConfig, ImageGenResult};

    #[tokio::test]
    async fn test_image_gen() -> crate::Result<()> {
        let config = VolcengineImageGenConfig::from_env()?;
        let websearch = config.try_into_image_gen().await?;
        let workspace: &'static Workspace = Box::leak(Box::new(Workspace::init("/tmp").await?));
        let ImageGenResult { images, .. } = websearch
            .generate(workspace, "一只阿拉蕾风格的兔子".into())
            .await?;
        for image in images {
            match image {
                Image::File(filepath) => println!("image-file: {}", filepath.display()),
            }
        }
        Ok(())
    }
}
