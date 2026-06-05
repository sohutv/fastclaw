use crate::agent::AgentId;
use crate::channels::SessionId;
use crate::channels::http_channel::type_::UserId;
use crate::channels::http_channel::{AppState, Transport};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Sse};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::ops::Deref;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone, Serialize, Deserialize)]
pub struct Params {
    user_id: UserId,
    /// default to main
    agent_id: Option<AgentId>,
}

pub async fn handle(
    State(AppState {
        channel,
        client,
        agent,
        ..
    }): State<AppState>,
    Query(Params { user_id, agent_id }): Query<Params>,
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
    let agent_id = agent.id();
    let rx = {
        let mut transports = client.write().await;
        let dst = transports
            .entry(user_id.clone())
            .or_insert(Default::default());
        let mut user_transports = dst.write().await;
        let vec = user_transports
            .entry(agent_id.clone())
            .or_insert(Default::default());
        let (transport, rx) = Transport::new(&session_id, agent_id.clone());
        vec.push(transport);
        rx
    };
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
    info!(
        "chat response transport connected, session_id: {session_id}, user_id: {user_id}, agent_id: {agent_id}"
    );
    Ok(response)
}
