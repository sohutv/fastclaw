use crate::agent::{AgentContext, AgentName};
use crate::channels::SessionId;
use crate::tools::{ToolCallError, ToolCallRsult};
use log::info;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct SessionHistoryBackupTool {
    ctx: Arc<AgentContext>,
}

impl SessionHistoryBackupTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    agent_name: AgentName,
    session_id: SessionId,
}

#[allow(async_fn_in_trait)]
impl Tool for SessionHistoryBackupTool {
    const NAME: &'static str = "session-history-backup";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Backup the history of a session for a given agent".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_name": {
                        "type": "string",
                        "description": "The name of the agent whose session history should be backed up",
                    },
                    "session_id": {
                        "type": "string",
                        "description": "The ID of the session whose history should be backed up",
                    },
                },
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        info!("Executing session-history-backup tool args: {:?}", args);
        let mut history_manager = self
            .ctx
            .history_manager
            .as_ref()
            .expect("history_manager is required")
            .write()
            .await;
        match history_manager
            .backup(&args.session_id, &args.agent_name)
            .await
        {
            Ok((path, timestamp)) => Ok(ToolCallRsult {
                success: true,
                output: serde_json::to_string(&json!({
                    "backup_filepath": path,
                    "backup_timestamp": timestamp,
                }))
                .map_err(|err| ToolCallError(format!("{err}")))?,
                error: None,
            }),
            Err(err) => Ok(ToolCallRsult {
                success: false,
                output: "".to_string(),
                error: Some(format!("{err}")),
            }),
        }
    }
}
