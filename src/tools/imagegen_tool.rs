use crate::agent::AgentContext;
use crate::service_provider::{Content, Image, ImageGenArgs, Images, StoreArgs, StoreResult};
use crate::tools::{ToolCallError, ToolCallRsult};
use itertools::Itertools;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct ImageGenTool {
    ctx: Arc<AgentContext>,
}

impl ImageGenTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    pub prompt: String,
    pub images: Option<Vec<String>>,
}

#[allow(async_fn_in_trait)]
impl Tool for ImageGenTool {
    const NAME: &'static str = "imagegen";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: r#"
Generate images from a text prompt.
- Optionally provide 1-14 reference images via `images` to guide generation.
- If an `images` item is a URL, it must be the full URL including all query parameters; do not truncate it.
- Returns local image file paths under the workspace downloads directory.
- Use this when the user asks to draw, design, or visualize ideas.
"#
            .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The text prompt used to generate images."
                    },
                    "images": {
                        "type": "array",
                        "description": "Optional reference images used to guide generation. Each item can be a local file path or image URL. If using URL, provide the complete URL including all query parameters; do not truncate it.",
                        "items": {
                            "type": "string"
                        },
                        "minItems": 1,
                        "maxItems": 14
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    async fn call(&self, Args { prompt, images }: Self::Args) -> Result<Self::Output, Self::Error> {
        let Some(image_gen_config) = &self.ctx.config.imagegen else {
            return Ok(ToolCallRsult::error("imagegen not configured"));
        };
        let imagegen = match image_gen_config.try_into_imagegen().await {
            Ok(it) => it,
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        };

        match imagegen
            .generate(
                self.ctx.workspace,
                ImageGenArgs {
                    prompt: prompt.into(),
                    images: if let Some(images) = images {
                        Some(
                            Images::try_from(&images)
                                .await
                                .map_err(|err| ToolCallError(format!("{err}")))?,
                        )
                    } else {
                        None
                    },
                },
            )
            .await
        {
            Ok(result) => {
                if result.images.is_empty() {
                    return Ok(ToolCallRsult::error("no images generated"));
                }
                let images = if let Some(storage_config) = &self.ctx.config.storage {
                    let storage = storage_config
                        .try_into_storage()
                        .await
                        .map_err(|err| ToolCallError(format!("{err}")))?;
                    let mut vec = Vec::with_capacity(result.images.len());
                    for image in result.images {
                        let url = match image {
                            Image::Url { url, .. } => url,
                            Image::File { path, format } => {
                                let StoreResult { signed_url, .. } = storage
                                    .store(
                                        self.ctx.workspace,
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
                                        self.ctx.workspace,
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
                        vec.push(url.to_string());
                    }
                    vec
                } else {
                    let mut vec = Vec::with_capacity(result.images.len());
                    for image in result.images {
                        let path = match image {
                            Image::Url { url, .. } => url.to_string(),
                            Image::File { path, .. } => path.display().to_string(),
                            Image::Raw { bytes, format } => {
                                let path = self.ctx.workspace.downloads_path.join(format!(
                                    "{}.{}",
                                    uuid::Uuid::new_v4(),
                                    format
                                ));
                                let _ = tokio::fs::write(&path, bytes)
                                    .await
                                    .map_err(|err| ToolCallError(format!("{err}")))?;
                                path.display().to_string()
                            }
                        };
                        let _ = vec.push(path);
                    }
                    vec
                };
                let output = images
                    .iter()
                    .enumerate()
                    .map(|(idx, image)| format!("- image {}: {}", idx + 1, image))
                    .join("\n");
                Ok(ToolCallRsult::ok(output))
            }
            Err(err) => Ok(ToolCallRsult::error(err.to_string())),
        }
    }
}
