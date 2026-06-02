use crate::agent::AgentId;
use crate::model_provider::ModelProviderName;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use crate::type_::{ModelName, Prompt, SystemPrompt};
use log::info;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub struct ForkChildAgentTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {
    id: Option<AgentId>,
    model_provider: ModelProviderName,
    model_name: ModelName,
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
                    "model_provider": {
                        "type": "string",
                        "description": "The name of the model provider to use for the new agent"
                    },
                    "model_name": {
                        "type": "string",
                        "description": "The name of the model to use for the new agent"
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
        info!(
            "Forking agent with model_provider: {:?}, model_name: {:?}",
            args.model_provider, args.model_name
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
            .fork_child(
                id.clone(),
                &args.model_provider,
                &args.model_name,
                Some(system_prompt),
            )
            .await
        {
            Ok(_) => Ok(ToolCallRsult {
                success: true,
                output: format!(
                    "Successfully forked agent with model provider {:?} and model {:?}",
                    args.model_provider, args.model_name
                ),
                error: None,
            }),
            Err(e) => Ok(ToolCallRsult {
                success: false,
                output: format!("Forked AgentId: {id}",),
                error: Some(format!("Failed to fork agent: {}", e)),
            }),
        }
    }
}
