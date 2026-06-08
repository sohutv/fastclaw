use crate::agent::{AgentGroup, AgentId};
use crate::channels::SessionId;
use crate::channels::http_channel::AppState;
use crate::channels::http_channel::type_::UserId;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::ops::Deref;

#[derive(Clone, Serialize, Deserialize)]
pub struct Params {
    user_id: UserId,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Args {
    pub(super) agent_group: AgentGroup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resp {
    pub(super) agent_id: AgentId,
}

pub async fn handle(
    State(app_state): State<AppState>,
    Query(Params { user_id }): Query<Params>,
    Json(Args { agent_group, .. }): Json<Args>,
) -> Result<Json<Resp>, StatusCode> {
    let _ = SessionId::try_from((user_id.deref(), &app_state.channel.config)).map_err(|err| {
        warn!("{err}");
        StatusCode::FORBIDDEN
    })?;
    let agent =
        super::get_or_create_if_not_present(&app_state, &user_id, None, Some(&agent_group)).await?;
    let agent_id = agent.id();
    info!("fork {agent_group} child agent for {user_id} ok, agent_id: {agent_id}");
    Ok(Json(Resp {
        agent_id: agent_id.clone(),
    }))
}
