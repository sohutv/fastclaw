use crate::agent::{AgentGroup, AgentId};
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use crate::type_::SystemPrompt;
use itertools::Itertools;
use log::{info, warn};
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
    description: String,
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
                        "agent_group": {
                            "type": "string",
                            "enum":  self.ctx.config.agent_groups.keys().collect_vec(),
                            "description": format!(
                r#"
The group to use for the new agent. This determines the agent's configuration profile including temperature, max tokens, and other behavioral parameters
{}
"#,
                self.ctx
                    .config
                    .agent_groups
                    .iter()
                    .map(|(k, v)| format!("- {}: {}", k, v))
                    .join("\n")
            ),
                        },
                        "system_prompt": {
                            "type": "string",
                            "description": "The system prompt that defines the behavior, personality, and instructions for the newly forked daemon agent. This will guide how the agent responds and what tasks it can perform",
                        },
                        "description":{
                             "type": "string",
                            "description": "A human-readable description of the purpose or role of this daemon agent. This helps identify what the agent is responsible for when listing or managing multiple agents"
                        }
                    },
                    "required": ["system_prompt", "agent_group","description"],
                }),
        }
    }

    async fn call(
        &self,
        Self::Args {
            agent_group,
            system_prompt,
            description,
        }: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        info!("Forking daemon agent, agent_group: {agent_group}, system_prompt: {system_prompt}",);
        let agent_id: AgentId = uuid::Uuid::new_v4().into();
        match self
            .ctx
            .parent_agent
            .fork_child(
                &agent_id,
                &agent_group,
                Some(system_prompt),
                Some(description),
            )
            .await
        {
            Ok(_) => {
                let _ = tokio::fs::write(
                    self.ctx
                        .agent_context()
                        .workspace
                        .agent_group_agent_lock_path(&agent_id)
                        .await
                        .map_err(|err| ToolCallError(format!("{err}")))?,
                    format!("{}", chrono::Local::now().timestamp_millis()).as_bytes(),
                )
                .await
                .map_err(|err| ToolCallError(format!("{err}")))?;
                Ok(ToolCallRsult {
                    success: true,
                    output: format!(
                        r#"
Forking daemon agent ok
- agent_id: `{}`
"#,
                        agent_id,
                    ),
                    error: None,
                })
            }
            Err(e) => {
                warn!("fork child agent failed: {e}");
                Ok(ToolCallRsult {
                    success: false,
                    output: Default::default(),
                    error: Some(format!("Failed to fork agent: {}", e)),
                })
            }
        }
    }
}
