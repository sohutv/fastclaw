use crate::agent::{AgentRequest, AgentResponse};
use crate::channels::ChannelMessage;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use crate::type_::Prompt;
use base64::Engine;
use rig::OneOrMany;
use rig::completion::{AssistantContent, ToolDefinition};
use rig::message::{DocumentSourceKind, ImageDetail, ImageMediaType, Message, UserContent};
use rig::tool::Tool;
use std::sync::Arc;

#[derive(Clone)]
pub struct ImageUnderstandingTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    pub prompt: Prompt,
    pub images: Vec<super::super::Image>,
}

#[allow(async_fn_in_trait)]
impl Tool for ImageUnderstandingTool {
    const NAME: &'static str = "image-understanding";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Understand and analyze images by providing a prompt and one or more images. Returns detailed analysis and description of the image content.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The prompt or question about the images to analyze"
                    },
                    "images": {
                        "type": "array",
                        "description": "Array of images to analyze, Each item can be a local file path or image URL. If using URL, provide the complete URL including all query parameters; do not truncate it.",
                        "items": {
                            "type": "string"
                        },
                        "minItems": 1,
                        "maxItems": 3
                    }
                },
                "required": ["prompt", "images"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let Self::Args { prompt, images } = args;

        let join_handle = {
            let session_id = self.ctx.session_id.clone();
            let agent = Arc::clone(&self.ctx.agent);
            let join_handle = tokio::spawn(async move {
                agent
                    .run(
                        AgentRequest {
                            id: uuid::Uuid::new_v4().into(),
                            session_id,
                            message: Message::User {
                                content: OneOrMany::many({
                                    let mut vec = Vec::with_capacity(images.len() + 1);
                                    vec.push(UserContent::text(prompt));
                                    for image in images {
                                        let data = {
                                            let image = image.try_into_image().await?;
                                            let data = image.as_png().await?;
                                            data
                                        };
                                        vec.push(UserContent::Image(
                                            rig::completion::message::Image {
                                                data: DocumentSourceKind::Base64(
                                                    base64::engine::general_purpose::STANDARD
                                                        .encode(&data),
                                                ),
                                                media_type: Some(ImageMediaType::PNG),
                                                detail: Some(ImageDetail::Auto),
                                                additional_params: None,
                                            },
                                        ));
                                    }
                                    vec
                                })?,
                            },
                        },
                        tx,
                        None,
                        Some(Box::new(|_| None)),
                    )
                    .await?;
                Ok::<_, anyhow::Error>(())
            });
            join_handle
        };

        let mut buff = vec![];
        while let Some(channel_message) = rx.recv().await {
            let ChannelMessage { message, .. } = channel_message;
            match message {
                AgentResponse::MessageStream(message) => match message {
                    Message::Assistant { content, .. } => {
                        for content in content.iter() {
                            match content {
                                AssistantContent::Text(text) => {
                                    let text_str = text.to_string();
                                    if !text_str.is_empty() {
                                        buff.push(text_str);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                },
                AgentResponse::Error(err) => {
                    return Err(ToolCallError(err));
                }
                _ => continue,
            }
        }
        // self.ctx.channel_message_sender.clone()
        match join_handle.await {
            Ok(_) => Ok(ToolCallRsult::ok(buff.join(""))),
            Err(err) => Err(ToolCallError(format!("{err}"))),
        }
    }
}
