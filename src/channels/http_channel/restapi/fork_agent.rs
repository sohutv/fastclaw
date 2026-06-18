use crate::agent::{AgentGroup, AgentId, OwnerSession};
use crate::channels::SessionId;
use crate::channels::http_channel::AppState;
use crate::channels::http_channel::type_::UserId;
use crate::type_::Preamble;
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
pub struct Body {
    agent_group: AgentGroup,
    prompt: Preamble,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resp {
    agent_group: AgentGroup,
    agent_id: AgentId,
}

pub async fn handle(
    State(app_state): State<AppState>,
    Query(Params { user_id }): Query<Params>,
    Json(Body {
        agent_group,
        prompt: system_prompt,
        ..
    }): Json<Body>,
) -> Result<Json<Resp>, StatusCode> {
    let session_id = SessionId::try_from((user_id.deref(), &app_state.channel.http_config)).map_err(|err| {
        warn!("{err}");
        StatusCode::FORBIDDEN
    })?;
    let agent = app_state
        .agent
        .fork_agent(
            &(uuid::Uuid::new_v4().into()),
            &agent_group,
            Some(system_prompt),
            None,
            &OwnerSession::Private(session_id.clone())
        )
        .await
        .map_err(|err| {
            warn!("fork child agent failed, agent_group: {agent_group}, err: {err}");
            StatusCode::BAD_REQUEST
        })?;
    let agent_id = agent.id();
    info!("fork {agent_group} child agent for {user_id} ok, agent_id: {agent_id}");
    Ok(Json(Resp {
        agent_group,
        agent_id: agent_id.clone(),
    }))
}
