use async_trait::async_trait;
use derive_more::Deref;
use rig::tool::ToolDyn;
use std::sync::Arc;

pub trait Filter {
    fn filter(&self, tool: Box<dyn ToolDyn>) -> Option<Box<dyn ToolDyn>>;
}

#[async_trait]
impl<F> Filter for F
where
    F: Fn(Box<dyn ToolDyn>) -> Option<Box<dyn ToolDyn>> + Sync + Send,
{
    fn filter(&self, tool: Box<dyn ToolDyn>) -> Option<Box<dyn ToolDyn>> {
        self(tool)
    }
}

#[derive(Clone, Deref)]
pub struct ToolFilter(Arc<dyn Filter + Send + Sync>);

impl Default for ToolFilter {
    fn default() -> Self {
        Self::from(|tool| Some(tool))
    }
}

impl<F> From<F> for ToolFilter
where
    F: Filter + Send + Sync + 'static,
{
    fn from(value: F) -> Self {
        Self(Arc::new(value))
    }
}

impl AsRef<Arc<dyn Filter + Sync + Send + 'static>> for ToolFilter {
    fn as_ref(&self) -> &Arc<dyn Filter + Sync + Send + 'static> {
        &self.0
    }
}

impl ToolFilter {
    pub fn and(self, other: ToolFilter) -> Self {
        Self(Arc::new(move |dst| {
            self.filter(dst).and_then(|dst| other.filter(dst))
        }))
    }
}

mod tool_name {
    use crate::tools::tool_filter::Filter;
    use derive_more::{Deref, From, FromStr};
    use rig::tool::ToolDyn;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum ToolNameFilter {
        #[serde(rename="accepts")]
        Accepts(Vec<ToolName>),
        #[serde(rename="rejects")]
        Rejects(Vec<ToolName>),
    }

    impl Default for ToolNameFilter {
        fn default() -> Self {
            Self::Rejects(vec![])
        }
    }

    impl Filter for ToolNameFilter {
        fn filter(&self, tool: Box<dyn ToolDyn>) -> Option<Box<dyn ToolDyn>> {
            let dst_tool_name = tool.name();
            match self {
                ToolNameFilter::Accepts(tool_names) => {
                    if tool_names
                        .iter()
                        .any(|it| it.eq_ignore_ascii_case(&dst_tool_name))
                    {
                        Some(tool)
                    } else {
                        None
                    }
                }
                ToolNameFilter::Rejects(tool_names) => {
                    if tool_names
                        .iter()
                        .any(|it| it.eq_ignore_ascii_case(&dst_tool_name))
                    {
                        None
                    } else {
                        Some(tool)
                    }
                }
            }
        }
    }

    #[derive(Debug, Clone, From, FromStr, Deref, Serialize, Deserialize)]
    pub struct ToolName(String);
}
pub use tool_name::*;
