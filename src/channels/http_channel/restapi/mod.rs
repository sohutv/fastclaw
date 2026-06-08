use crate::agent::{Agent, AgentGroup, AgentId, AgentSettings};
use crate::channels::http_channel::type_::UserId;
use crate::channels::http_channel::{AppState, HttpChannel};
use crate::model_provider::ModelProviderName;
use crate::type_::{ModelName, SystemPrompt};
use anyhow::anyhow;
use axum::http::StatusCode;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::sync::Arc;

pub mod chat_keepalive;
pub mod chat_request;
pub mod fork_agent;

pub mod drop_agent;
pub async fn get_or_create_if_not_present(
    AppState {
        channel,
        agent: main,
        ..
    }: &AppState,
    user_id: &UserId,
    agent_id: Option<&AgentId>,
    agent_group: Option<&AgentGroup>,
) -> Result<Arc<dyn Agent>, StatusCode> {
    if let Some(agent_id) = agent_id {
        if agent_id.deref().eq("main") {
            return Ok(Arc::clone(main));
        } else if let Some(agent) = main.context().children.read().await.get(agent_id) {
            return Ok(Arc::clone(agent));
        }
    }
    let Some(agent_group) = agent_group else {
        warn!("cannot create agent without agent_group, agent_id: {agent_id:?}");
        return Err(StatusCode::BAD_REQUEST);
    };
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
    let agent = main
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
    info!(
        "fork {agent_group} child agent for {user_id} ok, agent_id: {}",
        agent.id()
    );
    Ok(agent)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentConfig {
    model_provider: ModelProviderName,
    model: ModelName,
    agent_settings: AgentSettings,
}
