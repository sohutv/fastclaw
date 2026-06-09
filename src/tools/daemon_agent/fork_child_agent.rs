use crate::agent::{AgentGroup, AgentId};
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use crate::type_::SystemPrompt;
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
    agent_group: AgentGroup,
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
            description: "Fork a daemon agent with specified agent_group and system_prompt"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_group":{
                        "type": "string",
                        "enum":  self.ctx.config.agent_groups,
                        "description": "The group to use for the new agent. This determines the agent's configuration profile including temperature, max tokens, and other behavioral parameters"
                    },
                    "system_prompt": {
                        "type": "string",
                        "description": "The system prompt that defines the behavior, personality, and instructions for the newly forked daemon agent. This will guide how the agent responds and what tasks it can perform",
                    },
                },
                "required": ["system_prompt", "agent_group"],
            }),
        }
    }

    async fn call(
        &self,
        Self::Args {
            agent_group,
            system_prompt,
        }: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        info!(
            "Forking daemon agent, agent_group: {:?}, system_prompt: {:?}",
            system_prompt, system_prompt
        );
        let agent_id: AgentId = uuid::Uuid::new_v4().into();

        match self
            .ctx
            .parent_agent
            .fork_child(&agent_id, &agent_group, Some(system_prompt))
            .await
        {
            Ok(_) => Ok(ToolCallRsult {
                success: true,
                output: format!("Forking daemon agent ok, agent_id: `{agent_id}`",),
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
