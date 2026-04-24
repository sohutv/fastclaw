use crate::service_provider::LoadArgs;
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use base64::Engine;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub struct CloudStorageLoadTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    pub key: String,
    pub save_to: Option<String>,
}

#[allow(async_fn_in_trait)]
impl Tool for CloudStorageLoadTool {
    const NAME: &'static str = "cloud-storage-load";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: r#"
Load content from cloud storage.
- If `save_to` is set, content is written to a local file.
- Returns metadata and text/base64 preview of content.
"#
            .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Object key in cloud storage."
                    },
                    "save_to": {
                        "type": "string",
                        "description": "Optional local output path. Relative paths are resolved from workspace root."
                    }
                },
                "required": ["key"]
            }),
        }
    }

    async fn call(&self, Args { key, save_to }: Self::Args) -> Result<Self::Output, Self::Error> {
        let Some(storage_config) = &self.ctx.agent_context().config.storage else {
            return Ok(ToolCallRsult::error("storage not configured"));
        };
        let storage = match storage_config.try_into_storage().await {
            Ok(it) => it,
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        };
        let result = match storage
            .load(self.ctx.agent_context().workspace, LoadArgs::from(key))
            .await
        {
            Ok(it) => it,
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        };

        if let Some(path) = save_to {
            let path = std::path::PathBuf::from(path);
            let path = if path.is_relative() {
                self.ctx.agent_context().workspace.path.join(path)
            } else {
                path
            };
            if let Some(parent) = path.parent() {
                if let Err(err) = tokio::fs::create_dir_all(parent).await {
                    return Ok(ToolCallRsult::error(format!("failed to create dir: {err}")));
                }
            }
            if let Err(err) = tokio::fs::write(&path, &result.content).await {
                return Ok(ToolCallRsult::error(format!("failed to write file: {err}")));
            }
            return Ok(ToolCallRsult::ok(format!(
                r#"
- key: {}
- bytes: {}
- md5: {}
- saved_to: {}
"#,
                result.key.as_str(),
                result.content.len(),
                result.md5,
                path.display()
            )));
        }

        let preview = match String::from_utf8(result.content.clone()) {
            Ok(text) => {
                let text = if text.chars().count() > 2000 {
                    format!(
                        "{}...(truncated)",
                        text.chars().take(2000).collect::<String>()
                    )
                } else {
                    text
                };
                format!("text\n```text\n{}\n```", text)
            }
            Err(_) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&result.content);
                let b64 = if b64.len() > 2000 {
                    format!("{}...(truncated)", &b64[..2000])
                } else {
                    b64
                };
                format!("base64\n```text\n{}\n```", b64)
            }
        };
        Ok(ToolCallRsult::ok(format!(
            r#"
- key: {}
- bytes: {}
- md5: {}
- preview:
{}
"#,
            result.key.as_str(),
            result.content.len(),
            result.md5,
            preview
        )))
    }
}
