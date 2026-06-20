use crate::agent::{AgentId, AgentVisitor};
use crate::channels::SessionId;
use crate::channels::http_channel::AppState;
use crate::channels::http_channel::type_::UserId;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use std::ops::Deref;

#[derive(Clone, Serialize, Deserialize)]
pub struct Params {
    user_id: UserId,
    /// default to main
    agent_id: AgentId,
}

pub async fn handle(
    State(AppState {
        channel,
        client,
        agent,
        ..
    }): State<AppState>,
    Query(Params { user_id, agent_id }): Query<Params>,
) -> Result<(), StatusCode> {
    let _ = SessionId::try_from((user_id.deref(), channel.http_config)).map_err(|err| {
        warn!("{err}");
        StatusCode::FORBIDDEN
    })?;
    if let Some(transports) = client.read().await.get(&user_id) {
        if let Some(dst) = transports.write().await.remove(&agent_id) {
            for transport in dst {
                drop(transport)
            }
        }
    }
    let agent = agent
        .context()
        .agent_registry
        .drop(&agent_id)
        .await
        .map_err(|err| {
            error!("drop agent {agent_id} failed, err: {err}");
            StatusCode::BAD_REQUEST
        })?;
    log::info!("drop agent {} ok", agent.id());
    Ok(())
}
