use crate::agent::{Agent, AgentId};
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use axum::routing::{delete, get};
use axum::{Router, routing::post};
use log::{error, info, warn};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};

use crate::channels::http_channel::{HttpReqMessage, HttpRespMessage, UserId};
use derive_more::Deref;
use itertools::Itertools;
use tokio::sync::RwLock;

mod type_;
pub use type_::*;

mod handle_input_message;
mod recv_agent_message;

mod config;
pub use config::*;
mod restapi;

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
    agent_id: AgentId,
    #[allow(unused)]
    session_id: SessionId,
    #[deref]
    sender: Sender<HttpRespMessage>,
}

impl Transport {
    fn new(session_id: &SessionId, agent_id: AgentId) -> (Self, Receiver<HttpRespMessage>) {
        let (sender, rx) = tokio::sync::mpsc::channel(64);
        (
            Self {
                id: uuid::Uuid::new_v4(),
                session_id: session_id.clone(),
                agent_id,
                sender,
            },
            rx,
        )
    }
}

#[derive(Deref, Default)]
pub struct Client(RwLock<HashMap<UserId, Arc<RwLock<HashMap<AgentId, Vec<Transport>>>>>>);
#[derive(Clone)]
struct AppState {
    channel: Arc<HttpChannel>,
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
                .route("/channel/agent", post(restapi::fork_agent::handle))
                .route("/channel/agent", delete(restapi::drop_agent::handle))
                .route("/channel/chat", get(restapi::chat_keepalive::handle))
                .route(
                    "/channel/chat/:resp_type",
                    post(restapi::chat_request::handle),
                )
        }
        .with_state(AppState {
            channel: self_.clone(),
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
