use crate::tools::ToolCallRsult;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub(super) struct CurrentTimeTool;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {}

#[allow(async_fn_in_trait)]
impl Tool for CurrentTimeTool {
    const NAME: &'static str = "current-time";
    type Error = super::ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Returns the current time in Local".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
            }),
        }
    }

    async fn call(&self, _: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(ToolCallRsult {
            success: true,
            output: chrono::Local::now().to_rfc3339(),
            error: None,
        })
    }
}
