use crate::tools::{ToolCallError, ToolCallRsult, ToolContext};
use itertools::Itertools;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;

#[derive(Clone)]
pub(super) struct MemoryRecallTool {
    pub ctx: ToolContext,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    pub query: String,
    pub top_k: Option<usize>,
    pub dt: Option<String>,
}

#[allow(async_fn_in_trait)]
impl Tool for MemoryRecallTool {
    const NAME: &'static str = "memory_recall";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: r#"
### Recall from memory.
- Returns relevant historical conversation snippets from memory.
- Use this to find past interactions, knowledge, or information discussed before.
"#
            .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query to find relevant memory.",
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "The number of memory results to return. default to 5",
                    },
                    "dt": {
                        "type": "string",
                        "description": "The cutoff date for memory search. Format: YYYY-MM-DD HH:mm:ss, default to recent 3 days",
                    },
                },
                "required": ["query"],
            }),
        }
    }

    async fn call(
        &self,
        Args { query, top_k, dt }: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let memory_manager = &self.ctx.agent_context().memory_manager;
        let dt = dt.and_then(|dt_str| {
            chrono::NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%d %H:%M:%S")
                .ok()
                .and_then(|it| it.and_local_timezone(chrono::Local).single())
        });
        let search_result = memory_manager
            .search(&self.ctx.session_id, &query, top_k.unwrap_or(5), dt)
            .await;
        match search_result {
            Ok(search_result) => {
                let output = search_result
                    .iter()
                    .map(|item| {
                        format!(
                            r#"
### Memory Item ID: {}
- **File Reference**: {}
- **Content**
```
{:?}
```
                        "#,
                            item.id,
                            item.file_ref
                                .as_deref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            item.message
                        )
                    })
                    .join("\n");
                Ok(ToolCallRsult::ok(output))
            }
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        }
    }
}
