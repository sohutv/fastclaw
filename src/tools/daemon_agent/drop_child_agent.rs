use crate::agent::AgentId;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use log::info;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub struct DropChildAgentTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {
    agent_id: AgentId,
}

#[allow(async_fn_in_trait)]
impl Tool for DropChildAgentTool {
    const NAME: &'static str = "drop-daemon-agent";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Drop a child daemon agent by its ID".to_string(),
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
        let agent_lock_path = self
            .ctx
            .agent_context()
            .workspace
            .agent_group_agent_lock_path(&args.agent_id)
            .await;
        match self.ctx.parent_agent.drop_child(&args.agent_id).await {
            Ok(_) => {
                if let Ok(path) = agent_lock_path {
                    let _ = tokio::fs::remove_file(&path).await;
                }
                Ok(ToolCallRsult {
                    success: true,
                    output: format!("Successfully dropped agent with ID: {}", args.agent_id),
                    error: None,
                })
            }
            Err(e) => {
                if let Ok(path) = agent_lock_path {
                    let _ = tokio::fs::remove_file(&path).await;
                }
                Ok(ToolCallRsult {
                    success: false,
                    output: format!("Failed to drop agent with ID: {}", args.agent_id),
                    error: Some(format!("Error: {}", e)),
                })
            }
        }
    }
}
