use crate::channels::Notify;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub struct A2UserNotifyTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {
    message: String,
}

#[allow(async_fn_in_trait)]
impl Tool for A2UserNotifyTool {
    const NAME: &'static str = "agent2user-notify-tool";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "send message to user".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message content to send user."
                    }
                },
                "required": ["message"]
            }),
        }
    }

    async fn call(&self, Self::Args { message }: Self::Args) -> Result<Self::Output, Self::Error> {
        let notifiers = self.ctx.agent_context().channel_notifier.read().await;
        for notifier in notifiers.iter() {
            let agent_id = self.ctx.parent_agent.id();
            let _ = notifier
                .send(Notify {
                    agent_id: agent_id.clone(),
                    title: format!("Notify({agent_id})"),
                    content: message.clone(),
                })
                .await;
        }
        Ok(ToolCallRsult::ok("send message to user ok"))
    }
}
