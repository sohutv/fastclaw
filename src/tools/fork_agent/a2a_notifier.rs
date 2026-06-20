use crate::agent::{AgentId, AgentRequest};
use crate::channels::Channel;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use log::warn;
use rig::completion::ToolDefinition;
use rig::message::UserContent;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub struct A2ANotifyTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {
    target_agent_id: Option<AgentId>,
    message: String,
}

#[allow(async_fn_in_trait)]
impl Tool for A2ANotifyTool {
    const NAME: &'static str = "agent2agent-notify-tool";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "send message to target agent, default to main-agent".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "target_agent_id": {
                        "type": "string",
                        "description": "The ID of the target agent to send the message to. If not provided, defaults to main-agent."
                    },
                    "message": {
                        "type": "string",
                        "description": "The message content to send to the target agent."
                    }
                },
                "required": ["message"]
            }),
        }
    }

    async fn call(
        &self,
        Self::Args {
            target_agent_id,
            message,
        }: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let target_agent_id = target_agent_id.unwrap_or_default();
        match self.call_actual(&target_agent_id, &message).await {
            Ok(_) => Ok(ToolCallRsult::ok(format!(
                "send message to agent: {} ok",
                target_agent_id
            ))),
            Err(err) => {
                warn!(
                    "send message to agent failed, from: {}, to: {}",
                    self.ctx.parent_agent.id(),
                    target_agent_id,
                );
                Ok(ToolCallRsult {
                    success: false,
                    output: Default::default(),
                    error: Some(format!("{err}")),
                })
            }
        }
    }
}

impl A2ANotifyTool {
    async fn call_actual(&self, target_agent_id: &AgentId, message: &str) -> crate::Result<()> {
        let src_agent_id = self.ctx.parent_agent.id();
        let target_agent = self
            .ctx
            .agent_context()
            .agent_registry
            .get(&target_agent_id)
            .await?;
        let _ = self
            .ctx
            .agent_context()
            .a2a_channel
            .spawn_agent_request(AgentRequest::new(
                &self.ctx.parent_agent.owner_session().try_into()?,
                target_agent.id(),
                UserContent::text(format!(
                    r#"
# Response from Agent: {src_agent_id}
```markdown
{message}
```"#
                )),
            ))
            .await;
        Ok(())
    }
}
