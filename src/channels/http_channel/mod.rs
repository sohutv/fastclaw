use crate::agent::{Agent, AgentId, MainAgent};
use crate::channels::{
    AgentRespState, Channel, ChannelContext, ChannelMessage, ChannelNotifier, Notify, SessionId,
};
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
    context: &'static ChannelContext,
    pub http_config: &'static HttpChannelConfig,
    pub http_client: Arc<RwLock<Option<Arc<HttpClient>>>>,
    pub agent: Arc<MainAgent>,
}

impl HttpChannel {
    pub async fn new(
        context: &'static ChannelContext,
        agent: &Arc<MainAgent>,
    ) -> crate::Result<Self> {
        Ok(Self {
            context,
            http_config: context
                .config
                .http_config
                .as_ref()
                .ok_or_else(|| anyhow!("http_config not found"))?,
            http_client: Default::default(),
            agent: Arc::clone(agent),
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
pub struct HttpClient(RwLock<HashMap<UserId, Arc<RwLock<HashMap<AgentId, Vec<Transport>>>>>>);
#[derive(Clone)]
struct AppState {
    channel: &'static HttpChannel,
    agent: Arc<MainAgent>,
    client: Arc<HttpClient>,
}

#[async_trait]
impl Channel for HttpChannel {
    type Client = Arc<HttpClient>;
    type JoinHandle = tokio::task::JoinHandle<crate::Result<()>>;

    async fn start(
        &'static self,
    ) -> crate::Result<(&'static Self, ChannelNotifier, Self::JoinHandle)> {
        let mut guard = self.http_client.write().await;
        if guard.is_some() {
            return Err(anyhow!("channel had been already started!!!"));
        }
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
            channel: self,
            agent: Arc::clone(&self.agent),
            client: Arc::clone(&client),
        });
        let addr: SocketAddr = self.http_config.addr.parse()?;
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("HttpCompletable HTTP channel listening on {}", addr);

        let join_handle = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                error!("HttpCompletable channel server error: {}", err);
                return Err(anyhow!("HttpCompletable server error: {}", err));
            }
            Ok(())
        });
        let notifier = {
            let config = self.http_config;
            let client = Arc::clone(&client);
            let (rx, mut tx) = tokio::sync::mpsc::channel(32);
            tokio::spawn(async move {
                while let Some(Notify {
                    agent_id, content, ..
                }) = tx.recv().await
                {
                    let master_session_ids = config.master_session_ids();
                    for session_id in master_session_ids {
                        if session_id
                            .settings(config)
                            .map(|it| it.show_connected)
                            .unwrap_or(false)
                        {
                            if let Ok(message) =
                                Self::create_resp_messages_actual(&session_id, content.clone())
                            {
                                let _ = message.send(&client, session_id, &agent_id).await;
                            }
                        }
                    }
                }
            });
            rx.into()
        };
        *guard = Some(client);
        Ok((self, notifier, join_handle))
    }

    fn context(&self) -> &'static ChannelContext {
        self.context
    }

    async fn client(&self) -> crate::Result<Self::Client> {
        self.http_client
            .read()
            .await
            .as_ref()
            .map(|it| Arc::clone(it))
            .ok_or(anyhow!("channel not started"))
    }

    async fn handle_agent_message(
        &self,
        http_client: &Self::Client,
        _message_from: Arc<dyn Agent>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message_result) = receiver.recv().await {
            match message_result {
                Ok(message) => {
                    match self
                        .handle_agent_message_actual(http_client, &message, state, &mut buff)
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
        let arr = self
            .http_config
            .session_configs
            .iter()
            .map(|it| &it.session_id)
            .collect_vec();
        Ok(arr)
    }
}

impl HttpChannel {
    fn create_resp_messages<C: Into<Payload>>(
        _: &HttpClient,
        _: &dyn Agent,
        session_id: &SessionId,
        _: &ChannelContext,
        content: C,
    ) -> crate::Result<HttpRespMessage> {
        Self::create_resp_messages_actual(session_id, content)
    }

    fn create_resp_messages_actual<C: Into<Payload>>(
        session_id: &SessionId,
        content: C,
    ) -> crate::Result<HttpRespMessage> {
        match &session_id {
            SessionId::Master { .. } | SessionId::Anonymous { .. } => Ok(content.into().into()),
            SessionId::Group { .. } => Err(anyhow!(
                "send robot message to group is not supported by http"
            )),
        }
    }
}

impl HttpRespMessage {
    async fn send(self, client: &HttpClient, session_id: &SessionId, agent_id: &AgentId) {
        let user_id = UserId::from(session_id);
        if let Some(guard) = client.read().await.get(&user_id) {
            let mut user_transports = guard.write().await;
            if let Some((agent_id, agent_transports)) = user_transports.remove_entry(agent_id) {
                let mut updated = vec![];
                for transport in agent_transports {
                    let sender = &transport.sender;
                    if sender.is_closed() {
                        log::warn!(
                            "failed to send resp message, transport had been closed, user_id: {}, agent_id: {} ",
                            user_id,
                            agent_id
                        );
                    } else {
                        let _ = sender.send(self.clone()).await;
                        updated.push(transport)
                    }
                }
                if !updated.is_empty() {
                    user_transports.insert(agent_id, updated);
                }
            }
        }
    }
}
