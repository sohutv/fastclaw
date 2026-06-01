use crate::agent::Agent;
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Sse};
use axum::routing::get;
use axum::{Json, Router, routing::post};
use log::{error, info, warn};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::{Arc, Weak};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::channels::http_channel::{HttpReqMessage, HttpRespMessage, UserId};
use derive_more::Deref;
use futures_core::Stream;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

mod type_;
pub use type_::*;

mod handle_input_message;
mod recv_agent_message;

mod config;
use crate::hash_map;
pub use config::*;

pub struct HttpChannel {
    #[allow(dead_code)]
    pub ctx: Arc<ChannelContext>,
    pub config: HttpChannelConfig,
}

impl<S: AsRef<str>> TryFrom<(S, &HttpChannelConfig)> for SessionId {
    type Error = anyhow::Error;

    fn try_from((session_id_key, config): (S, &HttpChannelConfig)) -> Result<Self, Self::Error> {
        match config.allow_session_id(session_id_key.as_ref()) {
            Some(dst) => Ok(dst.clone()),
            None => Err(anyhow!(
                "session_id {} not allowed",
                session_id_key.as_ref()
            )),
        }
    }
}

impl HttpChannel {
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
                .http_config
                .clone()
                .ok_or_else(|| anyhow!("http_config not found"))?,
        })
    }
}

#[derive(Deref, Clone)]
pub struct Transport {
    #[allow(unused)]
    id: uuid::Uuid,
    #[allow(unused)]
    session_id: SessionId,
    #[deref]
    sender: Weak<Sender<HttpRespMessage>>,
}

impl Transport {
    fn new(session_id: &SessionId) -> (Self, Receiver<HttpRespMessage>) {
        let (sender, rx) = tokio::sync::mpsc::channel(64);
        (
            Self {
                id: uuid::Uuid::new_v4(),
                session_id: session_id.clone(),
                sender: {
                    let sender = Arc::new(sender);
                    let weak_sender = Arc::downgrade(&sender);
                    weak_sender
                },
            },
            rx,
        )
    }
}

#[derive(Deref, Default)]
pub struct Client(Mutex<HashMap<UserId, Vec<Transport>>>);
#[derive(Clone)]
struct AppState {
    http_channel: Arc<HttpChannel>,
    agent: Arc<dyn Agent>,
    client: Arc<Client>,
}

#[async_trait]
impl Channel for HttpChannel {
    type Client = Client;
    type InboundMessage = HttpReqMessage;
    type JoinHandle = tokio::task::JoinHandle<crate::Result<()>>;

    async fn start(
        self,
        agent: Arc<dyn Agent>,
    ) -> crate::Result<(Arc<Self>, Arc<Self::Client>, Self::JoinHandle)> {
        let self_ = Arc::new(self);
        let agent = Agent::start(Arc::clone(&agent)).await?;
        let client = Default::default();
        let app = {
            Router::new()
                .route("/channel/chat/send", post(Self::handle_chat_send))
                .route("/channel/chat/recv/sse", get(Self::handle_chat_recv_sse))
        }
        .with_state(AppState {
            http_channel: self_.clone(),
            agent,
            client: Arc::clone(&client),
        });
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

        Ok((self_, client, join_handle))
    }

    async fn handle_agent_message(
        &self,
        client: Arc<Client>,
        inbound_message: Option<Self::InboundMessage>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message_result) = receiver.recv().await {
            match message_result {
                Ok(message) => {
                    match self
                        .handle_agent_message_actual(
                            &client,
                            inbound_message.as_ref(),
                            &message,
                            state,
                            &mut buff,
                        )
                        .await
                    {
                        Ok(AgentRespState::Final) | Err(_) => {
                            state = AgentRespState::Wait;
                            buff.clear();
                        }
                        Ok(next) => {
                            state = next;
                        }
                    }
                }
                Err(err) => {
                    warn!("recv error channel message: {err}");
                }
            }
        }
        Ok(())
    }

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>> {
        let arr = self.config.allow_session_ids.values().collect_vec();
        Ok(arr)
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct ChatRecvSSEParams {
    user_id: UserId,
}

#[derive(Clone, Serialize, Deserialize)]
struct ChatSendParams {
    sse: bool,
}

impl HttpChannel {
    async fn handle_chat_recv_sse(
        State(AppState {
            http_channel,
            client,
            ..
        }): State<AppState>,
        Query(ChatRecvSSEParams { user_id }): Query<ChatRecvSSEParams>,
    ) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
        let session_id =
            SessionId::try_from((user_id.deref(), &http_channel.config)).map_err(|err| {
                warn!("handle_chat_recv failed, err: {err}");
                StatusCode::FORBIDDEN
            })?;
        let rx = {
            let mut transports = client.lock().await;
            let vec = transports.entry(user_id.clone()).or_insert(vec![]);
            let (transport, rx) = Transport::new(&session_id);
            vec.push(transport);
            rx
        };
        let stream = ReceiverStream::new(rx).map(|it| {
            Ok::<_, Infallible>(match serde_json::to_string(&it) {
                Ok(json) => Event::default().data(json),
                Err(err) => Event::default().event("error").data(err.to_string()),
            })
        });
        Ok(Sse::new(stream))
    }

    async fn handle_chat_send(
        State(AppState {
            http_channel,
            agent,
            client,
        }): State<AppState>,
        Query(ChatSendParams { sse }): Query<ChatSendParams>,
        Json(data): Json<HttpReqMessage>,
    ) -> Result<axum::response::Response, StatusCode> {
        let session_id = SessionId::try_from((data.user_id.deref(), &http_channel.config))
            .map_err(|err| {
                warn!("handle_chat_recv failed, err: {err}");
                StatusCode::FORBIDDEN
            })?;
        if sse {
            let transports_exist = {
                let transports = client.lock().await;
                transports.get(&data.user_id).is_some()
            };
            if !transports_exist {
                warn!(
                    "handle_chat_recv failed, transports not exist, user_id: {}",
                    &data.user_id
                );
                return Err(StatusCode::FORBIDDEN);
            }
            let _ = http_channel
                .handle_input_message(agent, session_id, client, data.clone())
                .await
                .map_err(|err| {
                    warn!("handle_chat_send failed, err: {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            Ok(StatusCode::OK.into_response())
        } else {
            let (transport, mut rx) = Transport::new(&session_id);
            let client = Arc::new(Client(Mutex::new(hash_map!(
                data.user_id.clone() => vec![transport],
            ))));
            let _ = http_channel
                .handle_input_message(agent, session_id, client, data.clone())
                .await
                .map_err(|err| {
                    warn!("handle_chat_send failed, err: {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            let mut payloads = vec![];
            while let Some(HttpRespMessage {
                payloads: mut array,
                ..
            }) = rx.recv().await
            {
                payloads.append(&mut array);
            }
            Ok(Json(HttpRespMessage {
                message_id: data.message_id,
                user_id: data.user_id,
                payloads,
            })
            .into_response())
        }
    }
}
