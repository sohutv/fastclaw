use crate::service_provider::{Content, Hdr, ImageEnhancerArgs, StoreArgs, StoreResult};
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use crate::type_::Image;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub struct ImageEnhancerTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    pub image: String,
    pub hdr: Option<Hdr>,
    pub wb: Option<bool>,
}

#[allow(async_fn_in_trait)]
impl Tool for ImageEnhancerTool {
    const NAME: &'static str = "image_enhancer";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: r#"
Enhance an image with optional HDR and white balance adjustments.
- Provide an image via `image` (local file path or image URL)
- Optionally enable HDR with `hdr` (strength 0.0-1.0)
- Optionally enable white balance with `wb`
- Returns the enhanced image
- Use this when the user asks to enhance, improve, or edit an image.
"#
            .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "image": {
                        "type": "string",
                        "description": "The image to enhance. Can be a local file path or image URL. If using URL, provide the complete URL including all query parameters."
                    },
                    "hdr": {
                        "type": "number",
                        "description": "Optional HDR strength (0.0 to 1.0). If provided, HDR enhancement will be enabled with the specified strength.",
                        "minimum": 0.0,
                        "maximum": 1.0
                    },
                    "wb": {
                        "type": "boolean",
                        "description": "Optional flag to enable white balance adjustment."
                    }
                },
                "required": ["image"]
            }),
        }
    }

    async fn call(&self, Args { image, hdr, wb }: Self::Args) -> Result<Self::Output, Self::Error> {
        let Some(image_enhancer_config) = &self.ctx.agent_context().config.image_enhancer else {
            return Ok(ToolCallRsult::error("image_enhancer not configured"));
        };
        let image_enhancer = match image_enhancer_config.try_into_image_enhancer().await {
            Ok(it) => it,
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        };

        let input_image = Image::try_from(image)
            .await
            .map_err(|err| ToolCallError(format!("{err}")))?;

        match image_enhancer
            .enhance(
                self.ctx.agent_context().workspace,
                ImageEnhancerArgs {
                    image: input_image,
                    hdr,
                    wb: wb.unwrap_or(false),
                },
            )
            .await
        {
            Ok(result) => {
                let image =
                    if let Some(storage_config) = &self.ctx.agent_context().config.storage {
                        let storage = storage_config
                            .try_into_storage()
                            .await
                            .map_err(|err| ToolCallError(format!("{err}")))?;
                        let url = match result.image {
                            Image::Url { url, .. } => url,
                            Image::File { path, format } => {
                                let StoreResult { signed_url, .. } = storage
                                    .store(
                                        self.ctx.agent_context().workspace,
                                        StoreArgs {
                                            key: format!("{}.{}", uuid::Uuid::new_v4(), format)
                                                .into(),
                                            mime: format.into(),
                                            content: Content::File(path),
                                        },
                                    )
                                    .await
                                    .map_err(|err| ToolCallError(format!("{err}")))?;
                                signed_url
                            }
                            Image::Raw { bytes, format } => {
                                let StoreResult { signed_url, .. } = storage
                                    .store(
                                        self.ctx.agent_context().workspace,
                                        StoreArgs {
                                            key: format!("{}.{}", uuid::Uuid::new_v4(), format)
                                                .into(),
                                            mime: format.into(),
                                            content: Content::Raw(bytes),
                                        },
                                    )
                                    .await
                                    .map_err(|err| ToolCallError(format!("{err}")))?;
                                signed_url
                            }
                        };
                        url.to_string()
                    } else {
                        match result.image {
                            Image::Url { url, .. } => url.to_string(),
                            Image::File { path, .. } => path.display().to_string(),
                            Image::Raw { bytes, format } => {
                                let path =
                                    self.ctx.agent_context().workspace.downloads_path.join(
                                        format!("{}.{}", uuid::Uuid::new_v4(), format),
                                    );
                                let _ = tokio::fs::write(&path, bytes)
                                    .await
                                    .map_err(|err| ToolCallError(format!("{err}")))?;
                                path.display().to_string()
                            }
                        }
                    };
                Ok(ToolCallRsult::ok(format!("Enhanced image: {}", image)))
            }
            Err(err) => Ok(ToolCallRsult::error(err.to_string())),
        }
    }
}
