use crate::agent::AgentId;
use crate::channels::http_channel::type_::UserId;
use crate::channels::http_channel::AppState;
use crate::channels::SessionId;
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
    let _ = SessionId::try_from((user_id.deref(), &channel.config)).map_err(|err| {
        warn!("{err}");
        StatusCode::FORBIDDEN
    })?;
    let mut guard = client.write().await;
    if let Some(transports) = guard
        .remove(&user_id)
        .and_then(|mut it| it.remove(&agent_id))
    {
        for transport in transports {
            drop(transport)
        }
    }
    let agent = agent.drop_child(&agent_id).await.map_err(|err| {
        error!("drop agent {agent_id} failed, err: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    log::info!("drop agent {} ok", agent.id());
    Ok(())
}
