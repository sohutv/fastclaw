use crate::agent::AgentContext;
use crate::tools::ToolCallRsult;
use log::info;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

#[derive(Clone)]
pub struct ShellTool {
    ctx: Arc<AgentContext>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
pub struct Args {
    command: String,
    approved: bool,
    relation_cmds: Vec<String>,
    relation_paths: Vec<String>,
    risk_level: super::RiskLevel,
    timeout: u64,
}

impl ShellTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}

#[allow(async_fn_in_trait)]
impl Tool for ShellTool {
    const NAME: &'static str = "shell";
    type Error = super::ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Execute a shell command in the workspace directory".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "approved": {
                        "type": "boolean",
                        "description": "Set true to explicitly approve medium/high-risk commands in supervised mode",
                        "default": false
                    },
                    "relation_cmds": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": r#"
#### A list of related command identifiers.

#### example 1:
- command: "cat TOOLS.md"
- relation_cmds: ["cat"]

#### example 2:
- command: "cat TOOLS.md|grep hello"
- relation_cmds: ["cat","grep"]
                        "#
                    },
                    "relation_paths": {
                        "type": "array",
                        "description": "A list of file paths or directory paths related to the command.",
                        "items": {
                            "type": "string"
                        }
                    },
                    "risk_level": {
                        "type": "string",
                        "enum": ["Low", "Medium", "High"],
                        "description": "The assessed risk level of the operation."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "The maximum execution time allowed for the command, in seconds."
                    }
                },
                "required": ["command", "approved", "relation_cmds", "relation_paths", "risk_level", "timeout"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        info!("Executing shell command: {:?}", args);
        let Args {
            command, timeout, ..
        } = &args;
        let output = tokio::time::timeout(
            Duration::from_secs(*timeout),
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&self.ctx.workspace.path)
                .output(),
        )
        .await;
        match output {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                // todo Truncate output to prevent OOM future
                Ok(ToolCallRsult {
                    success: output.status.success(),
                    output: stdout,
                    error: if stderr.is_empty() {
                        None
                    } else {
                        Some(stderr)
                    },
                })
            }
            Ok(Err(err)) => Ok(ToolCallRsult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute command: {err}")),
            }),
            Err(_) => Ok(ToolCallRsult {
                success: false,
                output: String::new(),
                error: Some(format!("Command timed out after {timeout}s and was killed",)),
            }),
        }
    }
}
