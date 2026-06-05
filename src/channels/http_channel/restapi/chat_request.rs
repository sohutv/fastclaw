use crate::agent::AgentId;
use crate::channels::SessionId;
use crate::channels::http_channel::type_::{HttpReqMessage, UserId};
use crate::channels::http_channel::{AppState, Client, Transport};
use crate::hash_map;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Sse};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone, Serialize, Deserialize)]
pub struct Param {
    user_id: UserId,
    /// default to main
    agent_id: Option<AgentId>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum ChatRespType {
    #[serde(rename = "completable")]
    Completable,
    #[serde(rename = "sse")]
    SSE,
    #[serde(rename = "streamable")]
    Streamable,
    #[serde(rename = "push")]
    Push,
}

pub async fn handle(
    State(AppState {
        channel,
        agent,
        client,
    }): State<AppState>,
    Path(resp_type): Path<ChatRespType>,
    Query(Param { user_id, agent_id }): Query<Param>,
    Json(data): Json<HttpReqMessage>,
) -> Result<axum::response::Response, StatusCode> {
    let session_id = SessionId::try_from((user_id.deref(), &channel.config)).map_err(|err| {
        warn!("{err}");
        StatusCode::FORBIDDEN
    })?;
    let agent = if let Some(agent_id) = &agent_id {
        if let Some(agent) = agent.context().children.read().await.get(agent_id) {
            Arc::clone(agent)
        } else {
            agent
        }
    } else {
        agent
    };
    let agent_id = agent_id.as_ref().unwrap_or(agent.id());
    info!(
        "recv agent request, session_id: {session_id}, user_id: {user_id}, agent_id: {agent_id}, message_id: {}",
        data.message_id
    );
    match resp_type {
        ChatRespType::SSE => {
            let transports_exist = {
                let transports = client.read().await;
                if let Some(dst) = transports.get(&user_id) {
                    let transports = dst.read().await;
                    transports.get(agent_id).is_some()
                } else {
                    false
                }
            };
            if !transports_exist {
                warn!(
                    "handle_chat_recv failed, transports not exist, session_id: {session_id}, user_id: {user_id}, agent_id: {agent_id}, message_id: {}",
                    data.message_id
                );
                return Err(StatusCode::FORBIDDEN);
            }
            let _ = channel
                .handle_input_message(agent, session_id, client, data.clone())
                .await
                .map_err(|err| {
                    warn!("handle_chat_send failed, err: {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            Ok(StatusCode::OK.into_response())
        }
        ChatRespType::Push => {
            let _ = channel
                .handle_input_message(agent, session_id, client, data.clone())
                .await
                .map_err(|err| {
                    warn!("handle_chat_send failed, err: {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            Ok(StatusCode::OK.into_response())
        }
        ChatRespType::Streamable => {
            let (transport, rx) = Transport::new(&session_id, agent_id.clone());
            let client = Arc::new(Client(RwLock::new(hash_map!(
                user_id.clone() => Arc::new(RwLock::new(hash_map!(agent_id.clone() => vec![transport],))),
            ))));
            let _ = channel
                .handle_input_message(agent, session_id, client, data.clone())
                .await
                .map_err(|err| {
                    warn!("handle_chat_send failed, err: {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            use futures_util::stream::StreamExt as _;
            let sse = Sse::new(
                ReceiverStream::new(rx).map(|it| Ok::<_, Infallible>(Event::default().data(&**it))),
            )
            .keep_alive(Default::default());
            let mut response = sse.into_response();
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::header::HeaderValue::from_static("text/event-stream; charset=utf-8"),
            );
            Ok(response)
        }
        ChatRespType::Completable => {
            let (transport, mut rx) = Transport::new(&session_id, agent_id.clone());
            let client = Arc::new(Client(RwLock::new(hash_map!(
                user_id.clone() => Arc::new(RwLock::new(hash_map!(agent_id.clone() => vec![transport],))),
            ))));
            let _ = channel
                .handle_input_message(agent, session_id, client, data.clone())
                .await
                .map_err(|err| {
                    warn!("handle_chat_send failed, err: {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            let mut payloads = vec![];
            while let Some(resp) = rx.recv().await {
                payloads.push(resp)
            }
            Ok(Json(payloads).into_response())
        }
    }
}
