use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Workspace {
    path: PathBuf,
}

impl TryFrom<&Path> for Workspace {
    type Error = anyhow::Error;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        Ok(Self { path: path.into() })
    }
}
