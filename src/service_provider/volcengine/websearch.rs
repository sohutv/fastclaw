//! VolcengineWebsearch
//! https://www.volcengine.com/docs/87772/2272953?lang=zh
//!

use crate::config::{ApiKey, ApiUrl};
use crate::service_provider::{
    AuthDegree, QueryArgs, Timerange, Websearch, WebsearchConfig, WebsearchResult,
    WebsearchResultContext,
};
use anyhow::anyhow;
use async_trait::async_trait;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolcengineWebsearchConfig {
    api_url: ApiUrl,
    api_key: ApiKey,
}

#[derive(Clone)]
pub struct VolcengineWebsearch {
    config: VolcengineWebsearchConfig,
}
#[async_trait]
impl WebsearchConfig for VolcengineWebsearchConfig {
    type T = VolcengineWebsearch;
    async fn try_into_websearch(&self) -> crate::Result<Self::T> {
        Ok(VolcengineWebsearch {
            config: self.clone(),
        })
    }
}

#[async_trait]
impl Websearch for VolcengineWebsearch {
    async fn search(&self, args: QueryArgs) -> crate::Result<WebsearchResult> {
        let QueryArgs {
            query,
            count,
            timerange: Timerange { from, to },
        } = &args;
        // YYYY-MM-DD..YYYY-MM-DD
        let timerange = format!("{}..{}", from.format("%Y-%m-%d"), to.format("%Y-%m-%d"));
        let response = reqwest::Client::builder()
            .build()?
            .post(self.config.api_url.as_str())
            .header(
                "Authorization",
                format!("Bearer {}", self.config.api_key.as_str()),
            )
            .header("Content-Type", "application/json")
            .json(&json!({
              "Query": &query,
              "SearchType": "web",
              "Count": count,
              "Filter": {
                "NeedContent": true,
                "NeedSummary": true,
                "NeedUrl":true
              },
              "NeedSummary": true,
              "TimeRange": &timerange,
            }
            ))
            .send()
            .await
            .map_err(|err| anyhow!(err))?
            .json::<Response>()
            .await
            .map_err(|err| anyhow!(err))?;
        let result = WebsearchResult::try_from(response.result)?;
        Ok(result)
    }
}

impl TryFrom<Result> for WebsearchResult {
    type Error = anyhow::Error;

    fn try_from(
        Result {
            search_context,
            result_count,
            web_results,
            ..
        }: Result,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            context: WebsearchResultContext {
                query: search_context.origin_query.into(),
                result_count,
            },
            result_items: web_results
                .into_iter()
                .flat_map(|it| it.try_into())
                .collect_vec(),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct Response {
    #[serde(rename = "Result")]
    result: Result,
}
#[derive(Serialize, Deserialize)]
struct Result {
    #[serde(rename = "ResultCount")]
    pub result_count: usize,
    #[serde(rename = "WebResults")]
    pub web_results: Vec<Item>,
    #[serde(rename = "SearchContext")]
    pub search_context: SearchContext,
    #[serde(rename = "TimeCost")]
    pub time_cost: u64,
}

#[derive(Serialize, Deserialize)]
struct SearchContext {
    #[serde(rename = "OriginQuery")]
    pub origin_query: String,
    #[serde(rename = "SearchType")]
    pub search_type: String,
}

#[derive(Serialize, Deserialize)]
struct Item {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "SortId")]
    pub sort_id: i64,
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "SiteName")]
    pub site_name: Option<String>,
    #[serde(rename = "Url")]
    pub url: Option<String>,
    #[serde(rename = "Snippet")]
    pub snippet: String,
    #[serde(rename = "Summary")]
    pub summary: String,
    #[serde(rename = "Content")]
    pub content: Option<String>,
    #[serde(rename = "PublishTime")]
    pub publish_time: String,
    #[serde(rename = "LogoUrl")]
    pub logo_url: String,
    #[serde(rename = "RankScore")]
    pub rank_score: f64,
    #[serde(rename = "AuthInfoDes")]
    pub auth_info_des: String,
    #[serde(rename = "AuthInfoLevel")]
    pub auth_info_level: i64,
}
// id,sort_id,title,site_name, url, snippet,content,..

impl TryFrom<Item> for super::super::WebsearchResultItem {
    type Error = anyhow::Error;

    // 权威度评级，对应权威度描述，包括：1 非常权威、2 正常权威、3 一般权威、4 一般不权威
    fn try_from(
        Item {
            id,
            sort_id,
            title,
            site_name,
            url,
            snippet,
            content,
            auth_info_level,
            ..
        }: Item,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            id,
            sort_id,
            title,
            site_name,
            url,
            snippet,
            content,
            auth_degree: {
                match auth_info_level {
                    1 => AuthDegree::Highly,
                    2 => AuthDegree::Moderately,
                    3 => AuthDegree::Generally,
                    4 => AuthDegree::Unreliable,
                    _ => AuthDegree::default(),
                }
            },
        })
    }
}

impl VolcengineWebsearchConfig {
    fn from_env() -> crate::Result<Self> {
        Ok(Self {
            api_url: ApiUrl::from_str(std::env::var("VOLCENGINE_WEBSEARCH_API_URL")?.as_str())?,
            api_key: std::env::var("VOLCENGINE_WEBSEARCH_API_KEY")?.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::service_provider::volcengine::websearch::VolcengineWebsearchConfig;
    use crate::service_provider::{Websearch, WebsearchConfig};

    #[tokio::test]
    async fn test_websearch() -> crate::Result<()> {
        let config = VolcengineWebsearchConfig::from_env()?;
        let websearch = config.try_into_websearch().await?;
        let result = websearch.search("热点事件".into()).await?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        Ok(())
    }
}
