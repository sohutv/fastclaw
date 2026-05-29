// http_completable channel
use crate::agent::{Agent, AgentRequest, AgentResponse};
use crate::channels::{Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use axum::{
    Json, Router,
    routing::post,
};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

use rig::OneOrMany;
use rig::completion::Message;
use rig::message::{AssistantContent, DocumentSourceKind, Image, ImageDetail, ImageMediaType, ReasoningContent, UserContent};
use base64::Engine;
use crate::type_::Images;

mod handle_input_message;
mod recv_agent_message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpCompletableConfig {
    pub addr: String,
    pub session_id: SessionId,
}

pub struct HttpCompletableChannel {
    #[allow(dead_code)]
    pub ctx: Arc<ChannelContext>,
    pub config: HttpCompletableConfig,
}

impl HttpCompletableChannel {
    pub async fn new(
        config: &'static Config,
        workspace: &'static Workspace,
    ) -> crate::Result<Self> {
        Ok(Self {
            ctx: Arc::new(ChannelContext {
                config: config.clone(),
                workspace,
            }),
            config: config
                .http_completable_config
                .clone()
                .ok_or_else(|| anyhow!("http_completable config not found"))?,
        })
    }
}



#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub message: String,
    pub reasoning: Option<String>,
    pub session_id: String,
}

fn clean_base64(data: String) -> String {
    if let Some(pos) = data.find(";base64,") {
        data[pos + 8..].to_string()
    } else {
        data
    }
}

#[async_trait]
impl Channel for HttpCompletableChannel {
    type Client = ();
    type JoinHandle = tokio::task::JoinHandle<crate::Result<()>>;

    async fn start(
        self,
        agent: Arc<dyn Agent>,
    ) -> crate::Result<(Arc<Self>, Arc<Self::Client>, Self::JoinHandle)> {
        let self_ = Arc::new(self);
        let _ = Agent::start(Arc::clone(&agent)).await?;

        let app = {
            let self_clone = Arc::clone(&self_);
            let agent_clone = Arc::clone(&agent);

            Router::new().route(
                "/channel/chat/completable",
                post(move |Json(body): Json<super::type_::HttpMessage>| {
                    let self_ = Arc::clone(&self_clone);
                    let agent = Arc::clone(&agent_clone);
                    async move {
                        let message = if let Some(images) = body.images {
                            if images.is_empty() {
                                Message::user(body.message)
                            } else {
                                let mut user_contents = vec![UserContent::text(body.message)];
                                let mut img_idx = 0usize;
                                for img in images {
                                    let is_url = img.data.starts_with("http://") || img.data.starts_with("https://");
                                    let image = if is_url {
                                        let bytes = match reqwest::get(&img.data).await {
                                            Ok(res) => match res.bytes().await {
                                                Ok(b) => b,
                                                Err(e) => {
                                                    log::warn!("Failed to get image bytes from url {}: {}", img.data, e);
                                                    continue;
                                                }
                                            },
                                            Err(e) => {
                                                log::warn!("Failed to download image from url {}: {}", img.data, e);
                                                continue;
                                            }
                                        };
                                        match image::load_from_memory(&bytes) {
                                            Ok(img) => img,
                                            Err(e) => {
                                                log::warn!("Failed to load image from memory for url {}: {}", img.data, e);
                                                continue;
                                            }
                                        }
                                    } else {
                                        let cleaned = clean_base64(img.data);
                                        let decoded = match base64::engine::general_purpose::STANDARD.decode(&cleaned) {
                                            Ok(bytes) => bytes,
                                            Err(e) => {
                                                log::warn!("Failed to decode base64 image: {}", e);
                                                continue;
                                            }
                                        };
                                        match image::load_from_memory(&decoded) {
                                            Ok(img) => img,
                                            Err(e) => {
                                                log::warn!("Failed to load image from base64 memory: {}", e);
                                                continue;
                                            }
                                        }
                                    };

                                    let mut image_data = vec![];
                                    let mut cursor = std::io::Cursor::new(&mut image_data);
                                    if let Err(e) = image.write_to(&mut cursor, image::ImageFormat::Png) {
                                        log::warn!("Failed to convert image to png: {}", e);
                                        continue;
                                    }

                                    let filename = format!("{}.png", uuid::Uuid::new_v4());
                                    let filepath = self_.ctx.workspace.downloads_path().join(&filename);
                                    if let Err(e) = tokio::fs::write(&filepath, &image_data).await {
                                        log::warn!("Failed to save image to path {}: {}", filepath.display(), e);
                                        continue;
                                    }

                                    img_idx += 1;
                                    user_contents.push(UserContent::Image(Image {
                                        data: DocumentSourceKind::Base64(
                                            base64::engine::general_purpose::STANDARD.encode(&image_data),
                                        ),
                                        media_type: Some(ImageMediaType::PNG),
                                        detail: Some(ImageDetail::Auto),
                                        additional_params: None,
                                    }));
                                    user_contents.push(UserContent::Text(
                                        format!(
                                            "- **filepath of the {}-th input image**: {}",
                                            img_idx,
                                            filepath.display()
                                        )
                                        .into(),
                                    ));
                                }
                                let content = match OneOrMany::many(user_contents) {
                                    Ok(val) => val,
                                    Err(_) => {
                                        return Err(axum::http::StatusCode::BAD_REQUEST);
                                    }
                                };
                                Message::User { content }
                            }
                        } else {
                            Message::user(body.message)
                        };

                        let session_id = match body.session_id {
                            Some(sid) => SessionId::Anonymous {
                                val: sid.into(),
                                settings: Default::default(),
                            },
                            None => self_.config.session_id.clone(),
                        };
                        let req = AgentRequest {
                            id: Default::default(),
                            session_id,
                            message,
                        };

                        match self_.spawn_agent_task(agent, req, body.addi_system_prompt).await {
                            Ok(mut receiver) => {
                                let mut accumulated_message = String::new();
                                let mut accumulated_reasoning = String::new();
                                let mut final_session_id = String::new();

                                while let Some(msg_res) = receiver.recv().await {
                                    match msg_res {
                                        Ok(msg) => {
                                            final_session_id = msg.session_id.to_string();
                                            match msg.message {
                                                AgentResponse::MessageStream(Message::Assistant { content, .. }) => {
                                                    for part in content {
                                                        if let AssistantContent::Text(text) = part {
                                                            accumulated_message.push_str(&text.to_string());
                                                        }
                                                    }
                                                }
                                                AgentResponse::ReasoningStream(reasoning) => {
                                                    for part in reasoning.content {
                                                        if let ReasoningContent::Text { text, .. } = part {
                                                            accumulated_reasoning.push_str(&text);
                                                        }
                                                    }
                                                }
                                                AgentResponse::Error(err) => {
                                                    error!("Agent returned error in stream: {}", err);
                                                    return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
                                                }
                                                _ => {}
                                            }
                                        }
                                        Err(e) => {
                                            error!("Error in agent task stream: {}", e);
                                            return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
                                        }
                                    }
                                }

                                let response = ChatResponse {
                                    message: accumulated_message,
                                    reasoning: if accumulated_reasoning.is_empty() { None } else { Some(accumulated_reasoning) },
                                    session_id: final_session_id,
                                };
                                Ok(Json(response))
                            }
                            Err(e) => {
                                error!("Failed to spawn agent task: {}", e);
                                Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                            }
                        }
                    }
                }),
            )
        };

        let addr: SocketAddr = self_.config.addr.parse()?;
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("HttpCompletable HTTP channel listening on {}", addr);

        let join_handle = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                error!("HttpCompletable channel server error: {}", err);
                return Err(anyhow!("HttpCompletable server error: {}", err));
            }
            Ok(())
        });

        Ok((self_, Arc::new(()), join_handle))
    }

    async fn handle_agent_message(
        &self,
        _client: Arc<Self::Client>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        while let Some(msg_result) = receiver.recv().await {
            match msg_result {
                Ok(msg) => {
                    info!("HttpCompletableChannel background task message: {:?}", msg.message);
                }
                Err(err) => {
                    error!("HttpCompletableChannel background task error: {}", err);
                }
            }
        }
        Ok(())
    }

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>> {
        Ok(vec![&self.config.session_id])
    }
}
