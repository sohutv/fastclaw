use crate::agent::{AgentGroup, AgentId};
use crate::model_provider::ModelPerformance;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use crate::type_::SystemPrompt;
use log::info;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use strum::IntoEnumIterator;

#[derive(Clone)]
pub struct ForkChildAgentTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {
    agent_group: AgentGroup,
    model: ModelPerformance,
    system_prompt: SystemPrompt,
}

#[allow(async_fn_in_trait)]
impl Tool for ForkChildAgentTool {
    const NAME: &'static str = "fork-daemon-agent";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Fork a daemon agent with specified model provider and model name"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "model": {
                        "type": "string",
                        "enum": ModelPerformance::iter().map(|it|it.to_string()).collect::<Vec<_>>(),
                        "description": "The performance level of the model to use for the new agent. This will automatically select an appropriate model provider and model name based on the specified performance tier"
                    },
                    "system_prompt": {
                        "type": "string",
                        "description": "The system prompt that defines the behavior, personality, and instructions for the newly forked daemon agent. This will guide how the agent responds and what tasks it can perform",
                    },
                    "agent_group":{
                        "type": "string",
                        "enum":  self.ctx.config.agent_groups,
                        "description": "The group to use for the new agent. This determines the agent's configuration profile including temperature, max tokens, and other behavioral parameters"
                    },
                },
                "required": ["model", "system_prompt", "agent_group"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (model_provider, model_name) = {
            let p = args.model;
            let dst = self
                .ctx
                .config
                .model_providers
                .iter()
                .flat_map(|(provider_name, it)| {
                    if let Some((model_name, _)) = it
                        .models()
                        .iter()
                        .filter(|(_, settings)| settings.performance.eq(&p))
                        .next()
                    {
                        Some((provider_name, model_name))
                    } else {
                        None
                    }
                })
                .next();
            if let Some(dst) = dst {
                dst
            } else {
                return Ok(Self::Output::error(format!(
                    "model not exist for performance: {}",
                    p
                )));
            }
        };
        info!(
            "Forking agent with model_provider: {:?}, model_name: {:?}",
            model_provider, model_name
        );
        let id: AgentId = uuid::Uuid::new_v4().into();
        let system_prompt = args.system_prompt.clone();
        let agent_settings = self
            .ctx
            .config
            .agent_settings(&args.agent_group)
            .unwrap_or(self.ctx.parent_agent.agent_settings());
        match self
            .ctx
            .parent_agent
            .fork_child(
                &id,
                &args.agent_group,
                model_provider,
                model_name,
                Some(system_prompt),
                agent_settings.clone(),
            )
            .await
        {
            Ok(_) => Ok(ToolCallRsult {
                success: true,
                output: format!(
                    "Successfully forked agent with model provider {model_provider} and model {model_name}, agent_id: `{id}`",
                ),
                error: None,
            }),
            Err(e) => Ok(ToolCallRsult {
                success: false,
                output: Default::default(),
                error: Some(format!("Failed to fork agent: {}", e)),
            }),
        }
    }
}
