use crate::config::Config;
use std::path::Path;

mod workspace;
pub use workspace::Workspace;

#[derive(Clone)]
pub struct Context {
    pub config: &'static Config,
    pub workspace: Workspace,
}

impl Context {
    pub fn new<P: AsRef<Path>>(
        config: &'static Config,
        workspace_path: P,
    ) -> crate::Result<Self> {
        Ok(Self {
            config,
            workspace: Workspace::try_from(workspace_path.as_ref())?,
        })
    }
}
