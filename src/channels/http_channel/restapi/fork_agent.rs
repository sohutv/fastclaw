use crate::agent::{AgentGroup, AgentId, AgentSettings};
use crate::channels::SessionId;
use crate::channels::http_channel::type_::UserId;
use crate::channels::http_channel::{AppState, HttpChannel};
use crate::model_provider::ModelProviderName;
use crate::type_::{ModelName, SystemPrompt};
use anyhow::anyhow;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::ops::Deref;

#[derive(Clone, Serialize, Deserialize)]
pub struct Params {
    user_id: UserId,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Args {
    agent_group: AgentGroup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentConfig {
    model_provider: ModelProviderName,
    model: ModelName,
    agent_settings: AgentSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resp {
    agent_id: AgentId,
}

pub async fn handle(
    State(AppState { channel, agent, .. }): State<AppState>,
    Query(Params { user_id }): Query<Params>,
    Json(Args { agent_group, .. }): Json<Args>,
) -> Result<Json<Resp>, StatusCode> {
    let _ = SessionId::try_from((user_id.deref(), &channel.config)).map_err(|err| {
        warn!("{err}");
        StatusCode::FORBIDDEN
    })?;
    if !channel.ctx.config.agent_groups.contains(&agent_group) {
        warn!("agent_group: {agent_group} not allow");
        return Err(StatusCode::FORBIDDEN);
    }
    let config = channel
        .get_agent_group_config(&agent_group)
        .await
        .map_err(|err| {
            warn!("{err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let system_prompt = channel
        .get_agent_group_system_prompt(&agent_group)
        .await
        .map_err(|err| {
            warn!("{err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let agent = agent
        .fork_child(
            &uuid::Uuid::new_v4().into(),
            &agent_group,
            &config.model_provider,
            &config.model,
            Some(system_prompt),
            config.agent_settings,
        )
        .await
        .map_err(|err| {
            error!("fork {agent_group} child agent failed, err: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let agent_id = agent.id();
    info!("fork {agent_group} child agent for {user_id} ok, agent_id: {agent_id}");
    Ok(Json(Resp {
        agent_id: agent_id.clone(),
    }))
}

impl HttpChannel {
    async fn get_agent_group_config(&self, agent_group: &AgentGroup) -> crate::Result<AgentConfig> {
        let agent_group_dir = self
            .ctx
            .workspace
            .agent_groups_path
            .join(agent_group.deref());
        let config_path = agent_group_dir.join("config.toml");
        if !config_path.exists() {
            return Err(anyhow!("agent_group: {agent_group} config not exist!!!"));
        }
        let string = tokio::fs::read_to_string(&config_path)
            .await
            .map_err(|err| anyhow!("read {agent_group} config failed, err: {err}"))?;
        let config = toml::from_str::<AgentConfig>(&string)
            .map_err(|err| anyhow!("parse {agent_group} config failed, err: {err}"))?;
        Ok(config)
    }

    async fn get_agent_group_system_prompt(
        &self,
        agent_group: &AgentGroup,
    ) -> crate::Result<SystemPrompt> {
        let agent_group_dir = self
            .ctx
            .workspace
            .agent_groups_path
            .join(agent_group.deref());
        let system_prompt = agent_group_dir.join("system_prompt.md");
        if !system_prompt.exists() {
            return Err(anyhow!(
                "agent_group: {agent_group} system_prompt not exist!!!"
            ));
        }
        let prompt = tokio::fs::read_to_string(&system_prompt)
            .await
            .map_err(|err| anyhow!("read {agent_group} system_prompt failed, err: {err}"))?;
        Ok(prompt.into())
    }
}
