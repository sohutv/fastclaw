use crate::agent::{
    Agent, AgentContext, AgentGroup, AgentId, AgentSettings, LlmAgentSupplier,
    OwnerSession, SystemPromptProvider,
};
use crate::config::Workspace;
use crate::model_provider::{ModelProviderName, ModelProviders};
use crate::type_::{ModelName, SystemPrompt};
use anyhow::anyhow;
use async_trait::async_trait;
use log::info;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub async fn reload_agent(
    agent_id: &AgentId,
    agent_context: &'static AgentContext,
) -> crate::Result<Arc<dyn Agent>> {
    let Ok(agent_config) = load_agent_config(agent_context.workspace, agent_id).await else {
        return Err(anyhow!(
            "reload_agent failed, agent_config not exist, agent_id: {agent_id}"
        ));
    };
    match spawn_agent_actual(agent_id, &agent_config, agent_context).await {
        Ok(agent) => {
            info!(
                "reload_agent ok, agent_id: {}, agent_group: {}",
                agent.id(),
                agent.agent_group()
            );
            Ok(agent)
        }
        Err(err) => {
            info!("reload_agent failed, agent_id: {agent_id}, err: {err}");
            Err(err)
        }
    }
}

pub async fn spawn_agent(
    agent_id: &AgentId,
    agent_group: &AgentGroup,
    addi_system_prompt: Option<SystemPrompt>,
    desc: Option<String>,
    owner_session: &OwnerSession,
    agent_context: &'static AgentContext,
) -> crate::Result<Arc<dyn Agent>> {
    let agent_config = if let Some(agent_config) =
        load_agent_config(agent_context.workspace, agent_id)
            .await
            .ok()
    {
        agent_config
    } else {
        if agent_context
            .config
            .agent_groups
            .get(&agent_group)
            .is_none()
        {
            return Err(anyhow!("agent_group: {agent_group} is forbidden"));
        }
        let config = AgentConfig {
            agent_group: agent_group.clone(),
            agent_group_config: get_agent_group_config(agent_context.workspace, &agent_group)
                .await?,
            addi_system_prompt,
            desc,
            owner_session: owner_session.clone(),
        };
        store_agent_config(agent_context.workspace, agent_id, agent_group, config).await?
    };
    spawn_agent_actual(agent_id, &agent_config, agent_context).await
}

static SPAWN_MAIN_AGENT_LOCK: AtomicBool = AtomicBool::new(false);

async fn spawn_agent_actual(
    agent_id: &AgentId,
    agent_config: &AgentConfig,
    agent_context: &'static AgentContext,
) -> crate::Result<Arc<dyn Agent>> {
    if agent_id.is_main() {
        if let Err(_) = SPAWN_MAIN_AGENT_LOCK.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            return Err(anyhow!("{agent_id} had been already spawned"));
        }
    }
    let AgentConfig {
        agent_group,
        agent_group_config:
            AgentGroupConfig {
                model,
                model_provider,
                agent_settings,
                ..
            },
        desc,
        owner_session,
        ..
    } = &agent_config;
    let agent = match agent_context.config.model_provider(model_provider)? {
        ModelProviders::OpenaiCompatible(model_provider) => {
            model_provider
                .create_agent(
                    agent_id,
                    agent_group,
                    model.clone(),
                    agent_settings.clone(),
                    desc.clone(),
                    owner_session,
                    Arc::new(SystemPromptProvider_ {
                        workspace: agent_context.workspace,
                        agent_config: agent_config.clone(),
                    }),
                    agent_context,
                )
                .await?
        }
    };
    Arc::new(agent).start().await
}

async fn agent_group_agent_config_path(
    workspace: &Workspace,
    agent_id: &AgentId,
) -> crate::Result<PathBuf> {
    let config_path = workspace
        .agent_group_agent_path(agent_id)
        .await?
        .join("config.toml");
    Ok(config_path)
}
async fn load_agent_config(
    workspace: &Workspace,
    agent_id: &AgentId,
) -> crate::Result<AgentConfig> {
    let config_path = agent_group_agent_config_path(workspace, agent_id).await?;
    if !config_path.exists() {
        return Err(anyhow!("agent config not exist, agent_id: {agent_id}"));
    }
    let string = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|err| anyhow!("read agent config failed, agent_id: {agent_id}, err: {err}"))?;
    let config = toml::from_str::<AgentConfig>(&string)
        .map_err(|err| anyhow!("parse agent config failed, agent_id: {agent_id},err: {err}"))?;
    Ok(config)
}

async fn store_agent_config(
    workspace: &Workspace,
    agent_id: &AgentId,
    agent_group: &AgentGroup,
    config: AgentConfig,
) -> crate::Result<AgentConfig> {
    match (
        agent_group.ignore_store(),
        agent_group_agent_config_path(workspace, agent_id).await,
    ) {
        (true, _) => Ok(config),
        (false, Ok(config_path)) => {
            if config_path.exists() {
                Err(anyhow!("agent config already exist, agent_id: {agent_id}"))
            } else {
                let string = toml::to_string_pretty(&config)?;
                tokio::fs::write(&config_path, &string).await?;
                Ok(config)
            }
        }
        (_, Err(err)) => Err(err),
    }
}

async fn get_agent_group_config(
    workspace: &Workspace,
    agent_group: &AgentGroup,
) -> crate::Result<AgentGroupConfig> {
    let config_path = workspace
        .agent_group_path(agent_group)
        .await?
        .join("config.toml");
    if !config_path.exists() {
        return Err(anyhow!("agent_group: {agent_group} config not exist!!!"));
    }
    let string = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|err| anyhow!("read {agent_group} config failed, err: {err}"))?;
    let config = toml::from_str::<AgentGroupConfig>(&string)
        .map_err(|err| anyhow!("parse {agent_group} config failed, err: {err}"))?;
    Ok(config)
}

async fn get_predefined_agent_group_system_prompt(
    workspace: &Workspace,
    agent_group: &AgentGroup,
    use_global_system_prompt: bool,
) -> crate::Result<Option<SystemPrompt>> {
    let system_prompt = workspace
        .agent_group_path(agent_group)
        .await?
        .join("system_prompt.md");
    match (
        use_global_system_prompt,
        tokio::fs::read_to_string(&system_prompt)
            .await
            .map(|it| SystemPrompt::from(it)),
    ) {
        (true, Ok(predefined)) => {
            let global = super::prompt::PromptSection::Identity
                .build(workspace)
                .await?;
            Ok(Some(global + predefined))
        }
        (false, Ok(predefined)) => Ok(Some(predefined)),
        (true, Err(_)) => {
            let global = super::prompt::PromptSection::Identity
                .build(workspace)
                .await?;
            Ok(Some(global))
        }
        (false, Err(_)) => Ok(None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentGroupConfig {
    model_provider: ModelProviderName,
    model: ModelName,
    agent_settings: AgentSettings,
    /// true if not present
    #[serde(default)]
    use_global_system_prompt: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentConfig {
    agent_group: AgentGroup,
    agent_group_config: AgentGroupConfig,
    addi_system_prompt: Option<SystemPrompt>,
    desc: Option<String>,
    owner_session: OwnerSession,
}

struct SystemPromptProvider_ {
    workspace: &'static Workspace,
    agent_config: AgentConfig,
}

#[async_trait]
impl SystemPromptProvider for SystemPromptProvider_ {
    async fn apply(&self) -> crate::Result<SystemPrompt> {
        let Self {
            workspace,
            agent_config:
                AgentConfig {
                    agent_group,
                    agent_group_config:
                        AgentGroupConfig {
                            use_global_system_prompt,
                            ..
                        },
                    addi_system_prompt,
                    ..
                },
        } = self;
        let predefined = get_predefined_agent_group_system_prompt(
            workspace,
            &agent_group,
            use_global_system_prompt.unwrap_or(true),
        )
        .await?;
        match (predefined, addi_system_prompt.clone()) {
            (Some(l), Some(r)) => Ok(l + r),
            (Some(it), _) | (_, Some(it)) => Ok(it),
            _ => Err(anyhow!("system prompt not exist!!!")),
        }
    }
}
