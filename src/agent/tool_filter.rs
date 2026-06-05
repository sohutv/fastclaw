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