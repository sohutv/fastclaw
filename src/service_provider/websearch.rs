use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::sync::Arc;
use strum::Display;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebsearchConfigs {
    #[cfg(feature = "volcengine")]
    #[serde(rename = "volcengine")]
    Volcengine(super::volcengine::websearch::VolcengineWebsearchConfig),
}

#[async_trait]
pub trait Websearch: Sync + Send {
    async fn search(&self, args: QueryArgs) -> crate::Result<WebsearchResult>;
}

#[async_trait]
pub trait WebsearchConfig: Sync + Send {
    type T: Websearch;
    async fn try_into_websearch(&self) -> crate::Result<Self::T>;
}

impl WebsearchConfigs {
    pub async fn try_into_websearch(&self) -> crate::Result<Arc<dyn Websearch>> {
        match self {
            #[cfg(feature = "volcengine")]
            WebsearchConfigs::Volcengine(config) => {
                let websearch = config.try_into_websearch().await?;
                Ok(Arc::new(websearch))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryArgs {
    pub query: Query,
    pub count: usize,
    pub timerange: Timerange,
}

impl<Q: Into<Query>> From<Q> for QueryArgs {
    fn from(value: Q) -> Self {
        Self {
            query: value.into(),
            count: 5,
            timerange: Default::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Timerange {
    pub from: chrono::NaiveDate,
    pub to: chrono::NaiveDate,
}

impl Default for Timerange {
    fn default() -> Self {
        let now = chrono::Local::now().naive_local().date();
        Self {
            from: now - chrono::Duration::days(7),
            to: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebsearchResult {
    pub context: WebsearchResultContext,
    pub result_items: Vec<WebsearchResultItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebsearchResultContext {
    pub query: Query,
    pub result_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query(String);

impl<S: Display> From<S> for Query {
    fn from(value: S) -> Self {
        Self(value.to_string())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WebsearchResultItem {
    pub id: String,
    pub sort_id: i64,
    pub title: String,
    pub site_name: Option<String>,
    pub url: Option<String>,
    pub snippet: String,
    pub content: Option<String>,
    pub auth_degree: AuthDegree,
}

/// Degree of authoritativeness
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Display, Default)]
pub enum AuthDegree {
    Highly,
    Moderately,
    Generally,
    #[default]
    Unreliable,
}
