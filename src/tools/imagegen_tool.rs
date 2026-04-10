use crate::agent::AgentContext;
use crate::service_provider::{Content, Image, StoreArgs, StoreResult};
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
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    async fn call(&self, Args { prompt }: Self::Args) -> Result<Self::Output, Self::Error> {
        let Some(image_gen_config) = &self.ctx.config.imagegen else {
            return Ok(ToolCallRsult::error("imagegen not configured"));
        };
        let imagegen = match image_gen_config.try_into_imagegen().await {
            Ok(it) => it,
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        };

        match imagegen.generate(self.ctx.workspace, prompt.into()).await {
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
                            Image::File { path, format } => {
                                let StoreResult { signed_url, .. } = storage
                                    .store(
                                        self.ctx.workspace,
                                        StoreArgs {
                                            key: path.display().to_string().into(),
                                            mime: format.into(),
                                            content: Content::File(path),
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
                    result
                        .images
                        .into_iter()
                        .map(|it| match it {
                            Image::File { path, .. } => path.display().to_string(),
                        })
                        .collect_vec()
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
