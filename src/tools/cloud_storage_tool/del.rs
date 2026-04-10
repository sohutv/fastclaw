use crate::agent::AgentContext;
use crate::service_provider::DelArgs;
use crate::tools::{ToolCallError, ToolCallRsult};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct CloudStorageDelTool {
    ctx: Arc<AgentContext>,
}

impl CloudStorageDelTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    pub key: String,
}

#[allow(async_fn_in_trait)]
impl Tool for CloudStorageDelTool {
    const NAME: &'static str = "cloud-storage-del";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Delete object from cloud storage by key.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Object key in cloud storage."
                    }
                },
                "required": ["key"]
            }),
        }
    }

    async fn call(&self, Args { key }: Self::Args) -> Result<Self::Output, Self::Error> {
        let Some(storage_config) = &self.ctx.config.storage else {
            return Ok(ToolCallRsult::error("storage not configured"));
        };
        let storage = match storage_config.try_into_storage().await {
            Ok(it) => it,
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        };
        match storage.del(self.ctx.workspace, DelArgs::from(key)).await {
            Ok(result) => Ok(ToolCallRsult::ok(format!(
                "deleted object key: {}",
                result.key.as_str()
            ))),
            Err(err) => Ok(ToolCallRsult::error(err.to_string())),
        }
    }
}
