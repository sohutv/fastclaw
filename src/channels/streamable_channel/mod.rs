// streamable_http channel

use crate::agent::{Agent, AgentRequest};
use crate::channels::{Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use axum::{
    Json, Router,
    response::sse::{Event, Sse},
    routing::post,
};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamableConfig {
    pub addr: String,
    pub session_id: SessionId,
}

pub struct StreamableChannel {
    #[allow(dead_code)]
    pub ctx: Arc<ChannelContext>,
    pub streamable_config: StreamableConfig,
}

impl StreamableChannel {
    pub async fn new(
        config: &'static Config,
        workspace: &'static Workspace,
    ) -> crate::Result<Self> {
        Ok(Self {
            ctx: Arc::new(ChannelContext {
                config: config.clone(),
                workspace,
            }),
            streamable_config: config
                .streamable_config
                .clone()
                .ok_or_else(|| anyhow!("streamable config not found"))?,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub session_id: Option<String>,
    pub addi_system_prompt: Option<String>,
}

#[async_trait]
impl Channel for StreamableChannel {
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
                "/chat",
                post(move |Json(body): Json<ChatRequest>| {
                    let self_ = Arc::clone(&self_clone);
                    let agent = Arc::clone(&agent_clone);
                    async move {
                        let message = rig::completion::Message::user(body.message);
                        let session_id = match body.session_id {
                            Some(sid) => SessionId::Anonymous {
                                val: sid.into(),
                                settings: Default::default(),
                            },
                            None => self_.streamable_config.session_id.clone(),
                        };
                        let req = AgentRequest {
                            id: Default::default(),
                            session_id,
                            message,
                        };

                        match self_.spawn_agent_task(agent, req, body.addi_system_prompt).await {
                            Ok(receiver) => {
                                let stream = ReceiverStream::new(receiver).map(|msg_res| {
                                    let event = match msg_res {
                                        Ok(msg) => match serde_json::to_string(&msg.message) {
                                            Ok(json) => Event::default().data(json),
                                            Err(e) => Event::default()
                                                .event("error")
                                                .data(e.to_string()),
                                        },
                                        Err(e) => Event::default()
                                            .event("error")
                                            .data(e.to_string()),
                                    };
                                    Ok::<_, std::convert::Infallible>(event)
                                });
                                Ok(Sse::new(stream))
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

        let addr: SocketAddr = self_.streamable_config.addr.parse()?;
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Streamable HTTP channel listening on {}", addr);

        let join_handle = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                error!("Streamable channel server error: {}", err);
                return Err(anyhow!("Streamable server error: {}", err));
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
                    info!("StreamableChannel background task message: {:?}", msg.message);
                }
                Err(err) => {
                    error!("StreamableChannel background task error: {}", err);
                }
            }
        }
        Ok(())
    }

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>> {
        Ok(vec![&self.streamable_config.session_id])
    }
}
