use crate::agent::{AgentContext, AgentSignal};
use crate::channels::{ChannelMessage, SessionId};
use crate::tools::ToolCallRsult;
use log::info;
use rig::completion::{Message, ToolDefinition};
use rig::tool::Tool;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct TaskCallbackTool {
    ctx: Arc<AgentContext>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {
    name: String,
    desc: String,
    #[serde(default)]
    session_id: SessionId,
    notify_message: String,
    risk_level: super::RiskLevel,
}

impl TaskCallbackTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}

impl Tool for TaskCallbackTool {
    const NAME: &'static str = "task-callback";
    type Error = super::ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Callback tool for task completion".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The name of the task"
                    },
                    "desc": {
                        "type": "string",
                        "description": "The description of the task"
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Task bind session_id"
                    },
                    "notify_message": {
                        "type": "string",
                        "description": "The message to notify after task completion"
                    },
                    "risk_level": {
                        "type": "string",
                        "enum": ["Low", "Medium", "High"],
                        "description": "The assessed risk level of the operation."
                    },
                },
                "required": ["name", "desc", "session_id", "notify_message", "risk_level"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        info!("Executing task-callback: {:?}", args);
        let session_id = args.session_id;
        if session_id.is_empty() {
            return Ok(ToolCallRsult {
                success: false,
                output: "".to_string(),
                error: Some("session_is is required".to_string()),
            });
        }
        let _ = self
            .ctx
            .channel_message_sender
            .send(ChannelMessage::Private {
                session_id,
                signal: AgentSignal::Message(Message::user(args.notify_message)),
            })
            .await;
        Ok(ToolCallRsult {
            success: true,
            output: "".to_string(),
            error: None,
        })
    }
}
