// http_completable channel
use crate::agent::Agent;
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router, routing::post};
use log::{error, info, warn};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};

use crate::channels::http_channel::{HttpReqMessage, HttpRespMessage, UserId};
use derive_more::Deref;
use itertools::Itertools;
use tokio::sync::Mutex;

mod type_;
pub use type_::*;

mod handle_input_message;
mod recv_agent_message;

mod config;
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
                .ok_or_else(|| anyhow!("http_completable config not found"))?,
        })
    }
}

pub struct Transport {
    #[allow(unused)]
    id: uuid::Uuid,
    #[allow(unused)]
    session_id: SessionId,
    tx: Sender<HttpRespMessage>,
    rx: Receiver<HttpRespMessage>,
}

impl Transport {
    fn new(session_id: &SessionId) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        Self {
            id: uuid::Uuid::new_v4(),
            session_id: session_id.clone(),
            tx,
            rx,
        }
    }
}

#[derive(Deref, Default)]
pub struct Client(Mutex<HashMap<UserId, Vec<Transport>>>);
#[derive(Clone)]
struct AppState {
    self_: Arc<HttpChannel>,
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
                .route("/channel/chat/recv", get(Self::handle_chat_recv))
        }
        .with_state(AppState {
            self_: self_.clone(),
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

impl HttpChannel {
    async fn handle_chat_recv(
        State(AppState { self_, client, .. }): State<AppState>,
        Query(user_id): Query<UserId>,
    ) -> Result<(), StatusCode> {
        let session_id = SessionId::try_from((user_id.deref(), &self_.config)).map_err(|err| {
            warn!("handle_chat_recv failed, err: {err}");
            StatusCode::FORBIDDEN
        })?;
        {
            let mut transports = client.lock().await;
            let vec = transports.entry(user_id.clone()).or_insert(vec![]);
            vec.push(Transport::new(&session_id));
        }
        todo!("send sse data");
        Ok(())
    }

    async fn handle_chat_send(
        State(AppState {
            self_,
            agent,
            client,
        }): State<AppState>,
        Json(data): Json<HttpReqMessage>,
    ) -> Result<(), StatusCode> {
        let user_id = &data.user_id;
        let session_id = SessionId::try_from((user_id.deref(), &self_.config)).map_err(|err| {
            warn!("handle_chat_recv failed, err: {err}");
            StatusCode::FORBIDDEN
        })?;
        let senders = {
            let transports = client.lock().await;
            let transport = transports.get(user_id).ok_or_else(|| {
                warn!("handle_chat_recv failed, transport not found, user_id: {user_id}");
                StatusCode::FORBIDDEN
            })?;
            Arc::new(transport.iter().map(|it| it.tx.clone()).collect_vec())
        };
        match self_
            .handle_input_message(agent, session_id, client, data)
            .await
        {
            Ok(()) => {
                todo!()
            }
            Err(err) => {
                todo!()
            }
        };
    }
}
