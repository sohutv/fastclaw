mod identity;
use crate::agent::AgentContext;
use derive_more::{Deref, Display, From, FromStr};
use identity::IdentityPrompt;

#[derive(Debug, Clone, Display, From, FromStr, Deref)]
pub struct Prompt(String);

#[derive(Debug, Clone, strum::EnumIter)]
pub enum PromptSection {
    Identity,
}

impl PromptSection {
    pub async fn build(&self, ctx: &AgentContext) -> crate::Result<Prompt> {
        let prompt = match self {
            PromptSection::Identity => IdentityPrompt.build(ctx).await?,
        };
        Ok(prompt)
    }
}
