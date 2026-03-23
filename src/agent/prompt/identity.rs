use crate::agent::AgentContext;
use crate::agent::prompt::Prompt;
use itertools::Itertools;
use std::path::Path;

pub struct IdentityPrompt;

const IDENTITY_MD_FILES: &[&str] = &[
    "AGENTS.md",
    "SOUL.md",
    "TOOLS.md",
    "IDENTITY.md",
    "USER.md",
    "HEARTBEAT.md",
    "BOOTSTRAP.md",
    "MEMORY.md",
    "cron/README.md",
];
impl IdentityPrompt {
    pub async fn build(
        &self,
        AgentContext { workspace, .. }: &AgentContext,
    ) -> crate::Result<Prompt> {
        Ok(self.build_actual(&workspace.path).await?)
    }

    async fn build_actual<P: AsRef<Path>>(&self, workspace_dir: P) -> crate::Result<Prompt> {
        let workspace_dir = workspace_dir.as_ref();
        let mut vec = Vec::with_capacity(IDENTITY_MD_FILES.len());
        for &filename in IDENTITY_MD_FILES {
            let filepath = workspace_dir.join(filename);
            let content = tokio::fs::read_to_string(filepath).await?;
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
        Ok(Prompt::from(prompt))
    }
}
