mod identity;

use crate::agent::AgentContext;
use crate::type_::SystemPrompt;
use identity::IdentityPrompt;

#[derive(Debug, Clone, strum::EnumIter)]
pub enum PromptSection {
    Identity,
}

impl PromptSection {
    pub async fn build(&self, ctx: &AgentContext) -> crate::Result<SystemPrompt> {
        let prompt = match self {
            PromptSection::Identity => IdentityPrompt.build(ctx).await?,
        };
        Ok(prompt)
    }
}
