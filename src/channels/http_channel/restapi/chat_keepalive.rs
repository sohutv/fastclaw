use crate::agent::{AgentGroup, AgentId};
use crate::channels::SessionId;
use crate::channels::http_channel::type_::UserId;
use crate::channels::http_channel::{AppState, Transport};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Sse};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone, Serialize, Deserialize)]
pub struct Params {
    user_id: UserId,
    #[serde(default)]
    agent_id: AgentId,
    agent_group: Option<AgentGroup>,
}

pub async fn handle(
    State(app_state): State<AppState>,
    Query(Params {
        user_id,
        agent_id,
        agent_group,
    }): Query<Params>,
) -> Result<axum::response::Response, StatusCode> {
    let session_id =
        SessionId::try_from((user_id.deref(), &app_state.channel.config)).map_err(|err| {
            warn!("{err}");
            StatusCode::FORBIDDEN
        })?;
    let agent = super::get_or_create_if_not_present(
        &app_state,
        &user_id,
        Some(&agent_id),
        agent_group.as_ref(),
    )
    .await?;
    let agent_id = agent.id();
    let rx = {
        let mut transports = app_state.client.write().await;
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
        ReceiverStream::new(rx).map(|it|{
            Event::default().json_data(it)
        }),
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
