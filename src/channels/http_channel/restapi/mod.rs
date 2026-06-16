use crate::agent::{Agent, AgentGroup, AgentId, OwnerSession};
use crate::channels::SessionId;
use crate::channels::http_channel::AppState;
use axum::http::StatusCode;
use log::{error, info};
use std::sync::Arc;

pub mod chat_keepalive;
pub mod chat_request;
pub mod fork_agent;

pub mod drop_agent;
pub async fn get_or_create(
    AppState { agent: main, .. }: &AppState,
    session_id: &SessionId,
    agent_id: &AgentId,
    agent_group: &AgentGroup,
) -> Result<Arc<dyn Agent>, StatusCode> {
    if agent_id.eq(main.id()) || agent_group.eq(main.agent_group()) {
        return Ok(Arc::clone(&main) as Arc<dyn Agent>);
    }
    let agent = main
        .fork_child(
            agent_id,
            agent_group,
            None,
            None,
            &OwnerSession::Private(session_id.clone()),
        )
        .await
        .map_err(|err| {
            error!("fork {agent_group} child agent failed, err: {err}");
            StatusCode::BAD_REQUEST
        })?;
    info!(
        "get {agent_group} child agent for {session_id} ok, agent_id: {}",
        agent.id()
    );
    Ok(agent)
}
