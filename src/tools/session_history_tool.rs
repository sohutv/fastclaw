use crate::agent::{AgentContext, AgentId};
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
pub struct BackupArgs {
    agent_id: AgentId,
    session_id: SessionId,
}

#[allow(async_fn_in_trait)]
impl Tool for SessionHistoryBackupTool {
    const NAME: &'static str = "session-history-backup";
    type Error = ToolCallError;
    type Args = BackupArgs;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: r#"
Archives the session history and resets the session state.
It automatically triggers a clear operation on the active session only after the backup is confirmed successfully
            "#.to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The id of the agent whose session history should be backed up",
                    },
                    "session_id": {
                        "type": "string",
                        "description": "The ID of the session whose history should be backed up",
                    },
                },
                "required": ["agent_id", "session_id"],
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
            .backup(&args.session_id, &args.agent_id)
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
