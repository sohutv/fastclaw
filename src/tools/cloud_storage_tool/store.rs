use crate::service_provider::{Content, StoreArgs};
use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use url::Url;

#[derive(Clone)]
pub struct CloudStorageStoreTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    pub key: String,
    pub mime: String,
    pub text_content: Option<String>,
    pub file_path: Option<String>,
    pub source_url: Option<String>,
    pub base64_content: Option<String>,
}

#[allow(async_fn_in_trait)]
impl Tool for CloudStorageStoreTool {
    const NAME: &'static str = "cloud-storage-store";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: r#"
Store content into configured cloud storage.
- Provide exactly one of: `text_content`, `file_path`, `source_url`, `base64_content`.
- Returns cloud object key and signed download URL.
"#
            .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Object key in cloud storage (e.g. docs/a.txt)."
                    },
                    "mime": {
                        "type": "string",
                        "description": "MIME type of the content (e.g. text/plain, image/png)."
                    },
                    "text_content": {
                        "type": "string",
                        "description": "Text content to upload."
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Local file path to upload. Relative paths are resolved from workspace root."
                    },
                    "source_url": {
                        "type": "string",
                        "description": "Public URL to fetch and upload."
                    },
                    "base64_content": {
                        "type": "string",
                        "description": "Base64 encoded bytes to upload."
                    }
                },
                "required": ["key", "mime"]
            }),
        }
    }

    async fn call(
        &self,
        Args {
            key,
            mime,
            text_content,
            file_path,
            source_url,
            base64_content,
        }: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let Some(storage_config) = &self.ctx.agent_context.config.storage else {
            return Ok(ToolCallRsult::error("storage not configured"));
        };
        let storage = match storage_config.try_into_storage().await {
            Ok(it) => it,
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        };
        let mime = match mime.parse() {
            Ok(it) => it,
            Err(err) => return Ok(ToolCallRsult::error(format!("invalid mime: {err}"))),
        };

        let content = {
            let has_text = text_content.is_some();
            let has_file = file_path.is_some();
            let has_url = source_url.is_some();
            let has_base64 = base64_content.is_some();
            let count = [has_text, has_file, has_url, has_base64]
                .iter()
                .filter(|it| **it)
                .count();
            if count == 0 {
                return Ok(ToolCallRsult::error(
                    "missing content source, provide one of text_content/file_path/source_url/base64_content",
                ));
            }
            if count > 1 {
                return Ok(ToolCallRsult::error(
                    "multiple content sources provided, provide exactly one",
                ));
            }
            if let Some(it) = text_content {
                Content::String(it)
            } else if let Some(it) = file_path {
                let path = std::path::PathBuf::from(&it);
                let path = if path.is_relative() {
                    self.ctx.agent_context.workspace.path.join(path)
                } else {
                    path
                };
                Content::File(path)
            } else if let Some(it) = source_url {
                let url = match Url::parse(&it) {
                    Ok(url) => url,
                    Err(err) => {
                        return Ok(ToolCallRsult::error(format!("invalid source_url: {err}")));
                    }
                };
                Content::Url(url)
            } else {
                let bytes = match base64_content {
                    Some(it) => {
                        match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, it)
                        {
                            Ok(bytes) => bytes,
                            Err(err) => {
                                return Ok(ToolCallRsult::error(format!(
                                    "invalid base64_content: {err}"
                                )));
                            }
                        }
                    }
                    None => unreachable!(),
                };
                Content::Raw(bytes)
            }
        };

        match storage
            .store(
                self.ctx.agent_context.workspace,
                StoreArgs {
                    key: key.into(),
                    mime,
                    content,
                },
            )
            .await
        {
            Ok(result) => Ok(ToolCallRsult::ok(format!(
                r#"
- key: {}
- signed_url: {}
"#,
                result.key.as_str(),
                result.signed_url
            ))),
            Err(err) => Ok(ToolCallRsult::error(err.to_string())),
        }
    }
}
