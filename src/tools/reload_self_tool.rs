use crate::agent::{AgentContext, AgentCtlSignal};
use crate::tools::{ToolCallError, ToolCallRsult};
use log::warn;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct ReloadSelfTool {
    ctx: Arc<AgentContext>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Args {
    reason: String,
}

impl ReloadSelfTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}

#[allow(async_fn_in_trait)]
impl Tool for ReloadSelfTool {
    const NAME: &'static str = "reload-self";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Reloads the agent(yourself) with the given reason if need".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "The reason for reloading the agent"
                    }
                },
                "required": ["reason"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let signal_id = uuid::Uuid::new_v4();
        warn!(
            "Reloading agent with reason: {}, signal_id: {}",
            args.reason, signal_id
        );
        let _ = self
            .ctx
            .ctl_signal_sender
            .send(AgentCtlSignal::Reload {
                id: signal_id.clone(),
                reason: args.reason,
            })
            .await;
        Ok(ToolCallRsult {
            success: true,
            output: format!(
                "already send reload signal to agent, signal_id: {}",
                signal_id
            ),
            error: None,
        })
    }
}
