use crate::agent::AgentContext;
use crate::agent::prompt::Prompt;
use itertools::Itertools;
use log::error;
use std::path::Path;

pub struct IdentityPrompt;

const IDENTITY_MD_FILES: &[(&str, &str)] = &[
    ("AGENTS.md", include_str!("../../../resources/AGENTS.md")),
    ("SOUL.md", include_str!("../../../resources/SOUL.md")),
    ("TOOLS.md", include_str!("../../../resources/TOOLS.md")),
    (
        "IDENTITY.md",
        include_str!("../../../resources/IDENTITY.md"),
    ),
    ("USER.md", include_str!("../../../resources/USER.md")),
    (
        "HEARTBEAT.md",
        include_str!("../../../resources/HEARTBEAT.md"),
    ),
    (
        "BOOTSTRAP.md",
        include_str!("../../../resources/BOOTSTRAP.md"),
    ),
    ("MEMORY.md", include_str!("../../../resources/MEMORY.md")),
    (
        "CRON_TASK.md",
        include_str!("../../../resources/CRON_TASK.md"),
    ),
];

impl IdentityPrompt {
    pub async fn build(
        &self,
        AgentContext { workspace, .. }: &AgentContext,
    ) -> crate::Result<Prompt> {
        let prompt = self.build_actual(&workspace.path).await?;
        Ok(prompt.into())
    }

    async fn build_actual(&self, workspace_dir: &Path) -> crate::Result<String> {
        let mut vec = Vec::with_capacity(IDENTITY_MD_FILES.len());
        for (filename, default_content) in IDENTITY_MD_FILES {
            let filepath = workspace_dir.join(filename);
            let content = if !filepath.exists() {
                let _ = tokio::fs::write(&filepath, default_content).await?;
                default_content.to_string()
            } else {
                tokio::fs::read_to_string(&filepath).await.map_err(|err| {
                    error!("Read file: {} failed, {err}", filepath.display());
                    err
                })?
            };
            vec.push((filename, content))
        }
        let prompt = vec
            .into_iter()
            .map(|(filename, content)| {
                format!(
                    r#"
### {}
{}
"#,
                    filename, content
                )
            })
            .join("\n");
        Ok(prompt)
    }
}
