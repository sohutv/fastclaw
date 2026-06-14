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
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub async fn reload_agent(
    config: &'static Config,
    history_manager: &Arc<dyn HistoryManager>,
    memory_manager: &Arc<MemoryManager>,
    workspace: &'static Workspace,
    mcp_registry: &'static McpRegistry,
    agent_id: &AgentId,
) -> crate::Result<Arc<dyn Agent>> {
    let Ok(agent_config) = get_agent_config(workspace, agent_id).await else {
        return Err(anyhow!(
            "reload_agent failed, agent_config not exist, agent_id: {agent_id}"
        ));
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
    let agent_config = if let Some(agent_config) = get_agent_config(workspace, agent_id).await.ok()
    {
        agent_config
    } else {
        if !config.agent_groups.contains(&agent_group) {
            return Err(anyhow!("agent_group: {agent_group} not allow"));
        }
        AgentConfig {
            agent_group: agent_group.clone(),
            agent_group_config: get_agent_group_config(workspace, &agent_group).await?,
            addi_system_prompt,
            desc,
        }
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
async fn spawn_agent_actual(
    config: &'static Config,
    history_manager: &Arc<dyn HistoryManager>,
    memory_manager: &Arc<MemoryManager>,
    workspace: &'static Workspace,
    mcp_registry: &'static McpRegistry,

    agent_id: &AgentId,
    agent_config: &AgentConfig,
) -> crate::Result<Arc<dyn Agent>> {
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
    Ok(agent)
}

async fn get_agent_config(workspace: &Workspace, agent_id: &AgentId) -> crate::Result<AgentConfig> {
    let config_path = workspace
        .agent_group_agent_path(agent_id)
        .await?
        .join("config.toml");
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
