use crate::agent::AgentContext;
use crate::service_provider::{Timerange, WebsearchQueryArgs};
use crate::tools::{ToolCallError, ToolCallRsult};
use chrono::Duration;
use itertools::Itertools;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct WebSearchTool {
    ctx: Arc<AgentContext>,
}

impl WebSearchTool {
    pub fn new(ctx: Arc<AgentContext>) -> crate::Result<Self> {
        Ok(Self { ctx })
    }
}
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Args {
    pub query: String,
    pub top_k: Option<usize>,
    pub timerange_from: Option<String>,
    pub timerange_to: Option<String>,
}

#[allow(async_fn_in_trait)]
impl Tool for WebSearchTool {
    const NAME: &'static str = "websearch";
    type Error = ToolCallError;
    type Args = Args;
    type Output = ToolCallRsult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: r#"
### Search the web for information.
- Returns relevant search results with titles, urls, contents.
- Use this to find current information, news, or research topics.
"#
            .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query. Be specific for better results.",
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "The number of search results to return. default to 5",
                    },
                    "timerange_from": {
                        "type": "string",
                        "description": "The start date for the search results. Format: YYYY-MM-DD, default to recent 7 days",
                    },
                    "timerange_to": {
                        "type": "string",
                        "description": "The end date for the search results. Format: YYYY-MM-DD, default to current date",
                    },
                },
                "required": ["query"],
            }),
        }
    }

    async fn call(
        &self,
        Args {
            query,
            top_k,
            timerange_from,
            timerange_to,
        }: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let Some(websearch_config) = &self.ctx.config.websearch else {
            return Ok(ToolCallRsult::error("websearch not configured"));
        };
        let websearch = match websearch_config.try_into_websearch().await {
            Ok(it) => it,
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        };
        let search_result = websearch
            .search(WebsearchQueryArgs {
                query: query.into(),
                count: top_k.unwrap_or(5),
                timerange: {
                    let timerange_from = if let Some(timerange_from) = timerange_from {
                        let Ok(timerange_from) =
                            chrono::NaiveDate::parse_from_str(&timerange_from, "%Y-%m-%d")
                        else {
                            return Ok(ToolCallRsult::error(
                                "Invalid timerange_from format. Expected format: YYYY-MM-DD",
                            ));
                        };
                        timerange_from
                    } else {
                        chrono::Local::now().date_naive() - Duration::days(7)
                    };
                    let timerange_to = if let Some(timerange_to) = timerange_to {
                        let Ok(timerange_to) =
                            chrono::NaiveDate::parse_from_str(&timerange_to, "%Y-%m-%d")
                        else {
                            return Ok(ToolCallRsult::error(
                                "Invalid timerange_to format. Expected format: YYYY-MM-DD",
                            ));
                        };
                        timerange_to
                    } else {
                        chrono::Local::now().date_naive()
                    };
                    Timerange {
                        from: timerange_from,
                        to: timerange_to,
                    }
                },
            })
            .await;
        match search_result {
            Ok(search_result) => {
                let output = search_result
                    .result_items
                    .into_iter()
                    .map(|ref item| {
                        format!(
                            r#"
### {} {}
- **ID**: {}
- **URL**: {}
- **Site Name**: {}
- **Snippet**: {}
- **Degree of Authoritativeness**: {}
- **Content**
```
{}
```
                        "#,
                            item.sort_id,
                            item.title,
                            item.id,
                            item.url.as_deref().unwrap_or_default(),
                            item.site_name.as_deref().unwrap_or_default(),
                            item.snippet,
                            item.auth_degree,
                            item.content.as_deref().unwrap_or_default()
                        )
                    })
                    .join("\n");
                Ok(ToolCallRsult::ok(output))
            }
            Err(err) => return Ok(ToolCallRsult::error(err.to_string())),
        }
    }
}
