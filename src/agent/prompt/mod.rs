mod identity;

use crate::type_::Preamble;
use identity::IdentityPrompt;
use crate::config::Workspace;

#[derive(Debug, Clone, strum::EnumIter)]
pub enum PromptSection {
    Identity,
}

impl PromptSection {
    pub async fn build(&self, workspace: &Workspace) -> crate::Result<Preamble> {
        let prompt = match self {
            PromptSection::Identity => IdentityPrompt.build(workspace).await?,
        };
        Ok(prompt)
    }
}
