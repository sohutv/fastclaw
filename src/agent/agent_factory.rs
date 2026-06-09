use crate::agent::{Agent, AgentGroup, AgentId, AgentSettings, HistoryManager, LlmAgentSupplier};
use crate::config::{Config, Workspace};
use crate::memory::MemoryManager;
use crate::model_provider::{ModelProviderName, ModelProviders};
use crate::tools::mcp_tool::McpRegistry;
use crate::type_::{ModelName, SystemPrompt};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
pub async fn spawn_agent<SPF, SPFut>(
    agent_id: &AgentId,
    agent_group: &AgentGroup,
    config: &'static Config,
    history_manager: &Arc<dyn HistoryManager>,
    memory_manager: &Arc<MemoryManager>,
    workspace: &'static Workspace,
    mcp_registry: &'static McpRegistry,
    system_prompt_supplier: SPF,
) -> crate::Result<Arc<dyn Agent>>
where
    SPFut: Future<Output = crate::Result<Option<SystemPrompt>>>,
    SPF: FnOnce(Option<SystemPrompt>) -> SPFut,
{
    if !config.agent_groups.contains(&agent_group) {
        return Err(anyhow!("agent_group: {agent_group} not allow"));
    }
    let AgentConfig {
        model,
        model_provider,
        agent_settings,
    } = get_agent_group_config(workspace, &agent_group).await?;
    let system_prompt = {
        let system_prompt_path = workspace
            .agent_group_agent_path(agent_group, agent_id)
            .await?
            .join("system_prompt.md");
        if let Ok(system_prompt) = tokio::fs::read_to_string(&system_prompt_path).await
            && !system_prompt.is_empty()
        {
            Some(system_prompt.into())
        } else {
            let predefined =
                get_predefined_agent_group_system_prompt(workspace, &agent_group).await?;
            let system_prompt = system_prompt_supplier(predefined).await?;
            if let Some(system_prompt) = &system_prompt {
                let _ = tokio::fs::write(&system_prompt_path, system_prompt.as_str()).await?;
            }
            system_prompt
        }
    };
    let agent = match config.model_provider(&model_provider)? {
        ModelProviders::OpenaiCompatible(model_provider) => {
            model_provider
                .create_agent(
                    agent_id,
                    agent_group,
                    config,
                    model,
                    Arc::clone(history_manager),
                    Arc::clone(memory_manager),
                    workspace,
                    system_prompt.clone(),
                    mcp_registry,
                    agent_settings,
                )
                .await?
        }
    };
    let agent = (Arc::new(agent) as Arc<dyn Agent>).start().await?;

    Ok(agent)
}
async fn get_agent_group_config(
    workspace: &Workspace,
    agent_group: &AgentGroup,
) -> crate::Result<AgentConfig> {
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
    let config = toml::from_str::<AgentConfig>(&string)
        .map_err(|err| anyhow!("parse {agent_group} config failed, err: {err}"))?;
    Ok(config)
}

async fn get_predefined_agent_group_system_prompt(
    workspace: &Workspace,
    agent_group: &AgentGroup,
) -> crate::Result<Option<SystemPrompt>> {
    let system_prompt = workspace
        .agent_group_path(agent_group)
        .await?
        .join("system_prompt.md");
    if !system_prompt.exists() {
        Ok(None)
    } else {
        let prompt = tokio::fs::read_to_string(&system_prompt)
            .await
            .map_err(|err| anyhow!("read {agent_group} system_prompt failed, err: {err}"))?;
        Ok(Some(prompt.into()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentConfig {
    model_provider: ModelProviderName,
    model: ModelName,
    agent_settings: AgentSettings,
}
