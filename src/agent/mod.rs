use derive_more::{Deref, Display, From, FromStr};
use rig::message::Message;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

mod openai_agent;

mod context;
use crate::agent::openai_agent::OpenaiAgent;
use crate::model_provider::{Model, ModelProvider};
pub use context::Context;

#[derive(Debug, Clone, From, FromStr, Deref, Eq, PartialEq, Ord, PartialOrd, Display)]
pub struct AgentName(String);

impl From<&str> for AgentName {
    fn from(value: &str) -> Self {
        AgentName(value.to_string())
    }
}

pub enum Agent {
    OpenaiAgent {
        delegate: OpenaiAgent,
        channel_sender: Sender<Message>,
    },
}

impl Agent {
    pub fn new<N: Into<AgentName>>(
        name: N,
        ctx: Arc<Context>,
        model_provider: ModelProvider,
        model: Model,
    ) -> crate::Result<Self> {
        match model_provider {
            ModelProvider::OpenaiCompatible(provider) => {
                let agent = OpenaiAgent::new(name.into(), ctx, provider, model)?;
                Ok(agent)
            }
        }
    }

    pub fn channel_sender(&self) -> Sender<Message> {
        match self {
            Agent::OpenaiAgent { channel_sender, .. } => channel_sender.clone(),
        }
    }

    pub async fn run(&mut self) -> crate::Result<()> {
        match self {
            Agent::OpenaiAgent { delegate, .. } => delegate.run().await,
        }
    }
}
