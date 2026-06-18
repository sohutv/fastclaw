use crate::agent::AgentId;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use log::info;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub struct DropAgentTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {
    agent_id: AgentId,
}

#[allow(async_fn_in_trait)]
impl Tool for DropAgentTool {
    const NAME: &'static str = "drop-agent";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Drop agent by its ID".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The ID of the agent to drop"
                    }
                },
                "required": ["agent_id"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        info!("Dropping agent with ID: {:?}", args.agent_id);
        match self.ctx.parent_agent.context().agent_registry.drop(&args.agent_id).await {
            Ok(_) => {
                Ok(ToolCallRsult {
                    success: true,
                    output: format!("Successfully dropped agent with ID: {}", args.agent_id),
                    error: None,
                })
            }
            Err(e) => {
                Ok(ToolCallRsult {
                    success: false,
                    output: format!("Failed to drop agent with ID: {}", args.agent_id),
                    error: Some(format!("Error: {}", e)),
                })
            }
        }
    }
}
