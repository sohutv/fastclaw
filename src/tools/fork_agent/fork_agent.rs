use crate::agent::{AgentGroup, AgentId, AgentRequest, OwnerSession};
use crate::channels::Channel;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use crate::type_::{Preamble, Prompt};
use itertools::Itertools;
use log::{info, warn};
use rig::completion::ToolDefinition;
use rig::message::UserContent;
use rig::tool::Tool;
use serde_json::json;
use std::ops::Deref;

#[derive(Clone)]
pub struct ForkAgentTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {
    agent_group: AgentGroup,
    preamble: Preamble,
    prompt: Prompt,
    description: String,
}

#[allow(async_fn_in_trait)]
impl Tool for ForkAgentTool {
    const NAME: &'static str = "fork-agent";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Fork a agent".to_string(),
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
                        "preamble": {
                            "type": "string",
                            "description": "The system prompt that defines the behavior, personality, and instructions for the newly forked agent. This will guide how the agent responds and what tasks it can perform",
                        },
                        "prompt": {
                            "type": "string",
                            "description": "The initial prompt or task to send to the newly forked agent immediately after creation. This is the first message the agent will process",
                        },
                        "description":{
                             "type": "string",
                            "description": "A human-readable description of the purpose or role of this agent. This helps identify what the agent is responsible for when listing or managing multiple agents"
                        }
                    },
                    "required": ["preamble", "agent_group", "prompt", "description"],
                }),
        }
    }

    async fn call(
        &self,
        Self::Args {
            agent_group,
            preamble,
            description,
            prompt,
        }: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        info!("fork agent, agent_group: {agent_group}, preamble: {preamble}",);
        let agent_id: AgentId = uuid::Uuid::new_v4().into();
        match self
            .ctx
            .parent_agent
            .fork_agent(
                &agent_id,
                &agent_group,
                Some(preamble),
                Some(description),
                &OwnerSession::Private(self.ctx.session_id.clone()),
            )
            .await
        {
            Ok(agent) => {
                let output = format!(
                    r#"
fork agent ok
- agent_id: `{}`
"#,
                    agent_id,
                );
                match self
                    .ctx
                    .parent_agent
                    .context()
                    .a2a_channel
                    .spawn_request(
                        &agent_id,
                        AgentRequest::new(&self.ctx.session_id, UserContent::text(prompt.deref())),
                    )
                    .await
                {
                    Ok(_) => Ok(ToolCallRsult {
                        success: true,
                        output,
                        error: None,
                    }),
                    Err(err) => {
                        let err_msg =
                            format!("fork agent ok, but send agent request failed: {err}");
                        warn!("{err_msg}");
                        Ok(ToolCallRsult {
                            success: true,
                            output,
                            error: Some(err_msg),
                        })
                    }
                }
            }
            Err(err) => {
                let err_msg = format!("fork agent failed: {err}");
                warn!("{err_msg}");
                Ok(ToolCallRsult {
                    success: false,
                    output: Default::default(),
                    error: Some(err_msg),
                })
            }
        }
    }
}
