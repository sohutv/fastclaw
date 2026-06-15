use crate::agent::{
    Agent, AgentGroup, AgentId, AgentSettings, HistoryManager, LlmAgentSupplier,
    SystemPromptProvider,
};
use crate::config::{Config, Workspace};
use crate::memory::MemoryManager;
use crate::model_provider::{ModelProviderName, ModelProviders};
use crate::tools::mcp_tool::McpRegistry;
use crate::type_::{ModelName, SystemPrompt};
use anyhow::anyhow;
use async_trait::async_trait;
use log::info;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub async fn reload_agent(
    config: &'static Config,
    history_manager: &Arc<dyn HistoryManager>,
    memory_manager: &Arc<MemoryManager>,
    workspace: &'static Workspace,
    mcp_registry: &'static McpRegistry,
    agent_id: &AgentId,
) -> crate::Result<Arc<dyn Agent>> {
    let Ok(agent_config) = load_agent_config(workspace, agent_id).await else {
        return Err(anyhow!(
            "reload_agent failed, agent_config not exist, agent_id: {agent_id}"
        ));
    };
    match spawn_agent_actual(
        config,
        history_manager,
        memory_manager,
        workspace,
        mcp_registry,
        agent_id,
        &agent_config,
    )
    .await
    {
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
    config: &'static Config,
    history_manager: &Arc<dyn HistoryManager>,
    memory_manager: &Arc<MemoryManager>,
    workspace: &'static Workspace,
    mcp_registry: &'static McpRegistry,

    agent_id: &AgentId,
    agent_group: &AgentGroup,
    addi_system_prompt: Option<SystemPrompt>,
    desc: Option<String>,
) -> crate::Result<Arc<dyn Agent>> {
    let agent_config = if let Some(agent_config) = load_agent_config(workspace, agent_id).await.ok()
    {
        agent_config
    } else {
        if config.agent_groups.get(&agent_group).is_none() {
            return Err(anyhow!("agent_group: {agent_group} is forbidden"));
        }
        let config = AgentConfig {
            agent_group: agent_group.clone(),
            agent_group_config: get_agent_group_config(workspace, &agent_group).await?,
            addi_system_prompt,
            desc,
        };
        store_agent_config(workspace, agent_id, agent_group, config).await?
    };
    spawn_agent_actual(
        config,
        history_manager,
        memory_manager,
        workspace,
        mcp_registry,
        agent_id,
        &agent_config,
    )
    .await
}

const SPAWN_MAIN_AGENT_LOCK: AtomicBool = AtomicBool::new(false);

async fn spawn_agent_actual(
    config: &'static Config,
    history_manager: &Arc<dyn HistoryManager>,
    memory_manager: &Arc<MemoryManager>,
    workspace: &'static Workspace,
    mcp_registry: &'static McpRegistry,

    agent_id: &AgentId,
    agent_config: &AgentConfig,
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
        ..
    } = &agent_config;
    let agent = match config.model_provider(model_provider)? {
        ModelProviders::OpenaiCompatible(model_provider) => {
            model_provider
                .create_agent(
                    agent_id,
                    agent_group,
                    config,
                    model.clone(),
                    Arc::clone(history_manager),
                    Arc::clone(memory_manager),
                    workspace,
                    Arc::new(SystemPromptProvider_ {
                        workspace,
                        agent_config: agent_config.clone(),
                    }),
                    mcp_registry,
                    agent_settings.clone(),
                    desc.clone(),
                )
                .await?
        }
    };
    let agent = (Arc::new(agent) as Arc<dyn Agent>).start().await?;
    if agent_id.is_main() {
        if let Ok(path) = workspace.agent_group_agents_path().await {
            if let Ok(mut dir) = tokio::fs::read_dir(&path).await {
                while let Ok(Some(dir_entry)) = dir.next_entry().await {
                    if let Ok(agent_id_str) = dir_entry.file_name().into_string() {
                        let agent_id = AgentId::from(agent_id_str);
                        if let Ok(agent_lock_path) =
                            workspace.agent_group_agent_lock_path(&agent_id).await
                        {
                            if agent_lock_path.exists() {
                                Box::pin(async {
                                    let _ = reload_agent(
                                        config,
                                        history_manager,
                                        memory_manager,
                                        workspace,
                                        mcp_registry,
                                        &agent_id,
                                    )
                                    .await;
                                })
                                .await;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(agent)
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
