//! ## Image Enhancer
//! [Wiki](https://www.volcengine.com/docs/86081/1660424?lang=zh)

use crate::config::{ApiKey, ApiUrl, Workspace};
use crate::service_provider::volcengine::request_sign::AuthHeader;
use crate::service_provider::{
    ImageEnhancer, ImageEnhancerArgs, ImageEnhancerConfig, ImageEnhancerResult,
};
use crate::type_::Image;
use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolcengineImageEnhancerConfig {
    api_url: ApiUrl,
    access_key: ApiKey,
    secret_key: ApiKey,
}

#[derive(Clone)]
pub struct VolcengineImageEnhancer {
    config: VolcengineImageEnhancerConfig,
}

#[async_trait]
impl ImageEnhancerConfig for VolcengineImageEnhancerConfig {
    type T = VolcengineImageEnhancer;

    async fn try_into_image_enhancer(&self) -> crate::Result<Self::T> {
        Ok(VolcengineImageEnhancer {
            config: self.clone(),
        })
    }
}

#[async_trait]
impl ImageEnhancer for VolcengineImageEnhancer {
    async fn enhance(
        &self,
        _: &'static Workspace,
        args: ImageEnhancerArgs,
    ) -> crate::Result<ImageEnhancerResult> {
        let ImageEnhancerArgs { image, hdr, wb } = &args;

        let url = {
            let mut url = url::Url::from_str(self.config.api_url.as_str())?;
            url.query_pairs_mut()
                .append_pair("Action", "CVProcess")
                .append_pair("Version", "2022-08-31")
                .finish();
            url
        };
        let b64_img = image.as_base64().await?.data();
        let body = {
            let body = json!({
               "req_key": "lens_lqir",
                "binary_data_base64": [b64_img],
                "enable_hdr": hdr.is_some(),
                "hdr_strength": hdr.as_ref().map(|it|it.strength()),
                "enable_wb": *wb,
                "result_format": 0, // 0 -> png, 1 -> jpeg
            });
            let body = serde_json::to_string(&body)?;
            body
        };
        let AuthHeader { x_date, auth, .. } = super::request_sign::create_auth_header(
            reqwest::Method::POST,
            &url,
            Some(body.as_bytes()),
            chrono::Utc::now(),
            "cn-north-1",
            "cv",
            self.config.access_key.as_str(),
            self.config.secret_key.as_str(),
        )?;
        let text = reqwest::Client::default()
            .post(url)
            .header("X-Date", &x_date)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?
            .text()
            .await?;

        let Response {
            resp_meta_data,
            data,
        } = serde_json::from_str::<Response>(&text)?;
        if let Some(ResponseMetadata { error }) = resp_meta_data {
            Err(anyhow!(
                "call volcengine image-enhancer failed, err_msg: {error:?}",
            ))
        } else {
            let Data {
                code,
                data,
                message,
            } = data.ok_or(anyhow!("unexpected data"))?;
            if let Some(b64) = data
                .as_ref()
                .map(|it| &it.binary_data_base64)
                .and_then(|it| it.first())
                .filter(|it| it.len() > 0)
            {
                Ok(ImageEnhancerResult {
                    image: Image::try_from(b64).await?,
                })
            } else {
                Err(anyhow!(
                    "call volcengine image-enhancer failed, code: {}, message: {}",
                    code,
                    message.as_deref().unwrap_or("unknown")
                ))
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Response {
    #[serde(rename = "ResponseMetadata")]
    resp_meta_data: Option<ResponseMetadata>,
    #[serde(flatten)]
    data: Option<Data>,
}
#[derive(Serialize, Deserialize)]
struct Data {
    code: i32,
    data: Option<ImageData>,
    message: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ResponseMetadata {
    #[serde(rename = "Error")]
    error: ResponseMetadataError,
}
#[derive(Debug, Serialize, Deserialize)]
struct ResponseMetadataError {
    #[serde(rename = "CodeN")]
    code_num: i32,
    #[serde(rename = "Code")]
    code: String,
    #[serde(rename = "Message")]
    message: String,
}

#[derive(Serialize, Deserialize)]
struct ImageData {
    binary_data_base64: Vec<String>,
}

impl VolcengineImageEnhancerConfig {
    #[allow(unused)]
    fn from_env() -> crate::Result<Self> {
        Ok(Self {
            api_url: ApiUrl::from_str(
                std::env::var("VOLCENGINE_IMAGE_ENHANCER_API_URL")?.as_str(),
            )?,
            access_key: std::env::var("VOLCENGINE_IMAGE_ENHANCER_ACCESS_KEY")?.into(),
            secret_key: std::env::var("VOLCENGINE_IMAGE_ENHANCER_SECRET_KEY")?.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Workspace;
    use crate::service_provider::volcengine::image_enhancer::VolcengineImageEnhancerConfig;
    use crate::service_provider::{ImageEnhancer, ImageEnhancerConfig, ImageEnhancerResult};
    use crate::type_::{Image, ImgFormat};
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::str::FromStr;

    const PATH: &str = "/Users/zhaowenhao/Desktop";
    const FILE: &str = "3853f48f-a9da-4e2a-a7fb-3f1e91ccb7d3.png";
    #[tokio::test]
    async fn test() -> crate::Result<()> {
        let config = VolcengineImageEnhancerConfig::from_env()?;
        let enhancer = config.try_into_image_enhancer().await?;
        let workspace: &'static Workspace = Box::leak(Box::new(Workspace::init("/tmp").await?));

        let ImageEnhancerResult { image, .. } = enhancer
            .enhance(
                workspace,
                Image::File {
                    path: PathBuf::from_str(PATH)?.join(FILE),
                    format: ImgFormat::Png,
                }
                .into(),
            )
            .await?;
        let image = image.as_image().await?;
        let mut data = vec![];
        let mut cursor = Cursor::new(&mut data);
        let _ = image.write_to(&mut cursor, image::ImageFormat::Png)?;
        let _ = tokio::fs::write(
            PathBuf::from_str(PATH)?.join(format!("enhanced_{}", FILE)),
            data,
        )
        .await?;
        Ok(())
    }
}
