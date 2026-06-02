use crate::agent::AgentId;
use crate::model_provider::ModelPerformance;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use crate::type_::{Prompt, SystemPrompt};
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
    id: Option<AgentId>,
    model: Option<ModelPerformance>,
    skill_names: Vec<Prompt>,
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
                    "id":{
                        "type": "string",
                        "description": "Optional ID for the new agent. If not provided, a UUID will be generated"
                    },
                    "model": {
                        "type": "string",
                        "enum": ModelPerformance::iter().map(|it|it.to_string()).collect::<Vec<_>>(),
                        "description": "The performance level of the model to use for the new agent. This will automatically select an appropriate model provider and model name based on the specified performance tier"
                    },
                    "skill_names": {
                        "type": "array",
                        "description": "Optional list of skill names to enable for the new agent",
                        "items": {
                            "type": "string"
                        }
                    }
                },
                "required": ["model_provider", "model_name"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (model_provider, model_name) = {
            let p = &args.model.unwrap_or_default();
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
        let id: AgentId = args.id.unwrap_or_else(|| uuid::Uuid::new_v4().into());
        let system_prompt = SystemPrompt::from("").append_line(
            args.skill_names
                .into_iter()
                .reduce(|l, r| l.append_line(r))
                .unwrap_or_default(),
        );
        match self
            .ctx
            .parent_agent
            .fork_child(id.clone(), model_provider, model_name, Some(system_prompt))
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
