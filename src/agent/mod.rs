use crate::agent::llm_agent::LlmAgent;
use crate::channels::{ChannelMessageReceiver, ChannelMessageSender, SessionId};
use derive_more::{Deref, Display, From, FromStr};
use rig::completion::Usage;
use rig::message::{Message, Reasoning, ToolCall};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

mod llm_agent;

mod prompt;

use crate::config::Config;
use crate::model_provider::{ModelName, ModelProviders};

#[allow(unused)]
#[derive(Clone)]
pub struct AgentContext {
    pub config: &'static Config,
    pub workspace: Workspace,
    pub msg_sender: AgentMessageSender,
    pub ctl_signal_sender: AgentCtlSignalSender,
    pub channel_message_sender: ChannelMessageSender,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: PathBuf,
}

impl<P: AsRef<Path>> From<P> for Workspace {
    fn from(value: P) -> Self {
        Self {
            path: value.as_ref().join("workspace"),
        }
    }
}

#[derive(Debug, Clone, From, FromStr, Deref, Eq, PartialEq, Ord, PartialOrd, Display)]
pub struct AgentName(String);

impl From<&str> for AgentName {
    fn from(value: &str) -> Self {
        AgentName(value.to_string())
    }
}

pub trait Agent {
    fn run(self: Box<Self>) -> crate::Result<JoinHandle<()>>;

    fn msg_sender(&self) -> AgentMessageSender;
}

pub async fn create_agent<N: Into<AgentName>, WorkDir: AsRef<Path>>(
    name: N,
    config: &'static Config,
    workdir: WorkDir,
    model_provider: ModelProviders,
    model: ModelName,
) -> crate::Result<(Box<dyn Agent>, ChannelMessageReceiver)> {
    match model_provider {
        ModelProviders::OpenaiCompatible(provider) => {
            let (agent, receiver) = LlmAgent::new(name, config, workdir, provider, model).await?;
            Ok((Box::new(agent), receiver))
        }
    }
}

#[derive(Clone, Deref, From)]
pub struct AgentMessageSender(Sender<AgentMessage>);

#[derive(Clone)]
pub enum AgentMessage {
    Private {
        session_id: SessionId,
        message: Message,
    },
    Group {
        message: Message,
    },
}

#[derive(Clone)]
pub enum AgentSignal {
    Start,
    ToolCall(ToolCall),
    ReasoningStream(Reasoning),
    MessageStream(Message),
    Final(Usage),
    Message(Message),
    Error(String),
}

#[derive(Debug, Clone)]
pub enum AgentCtlSignal {
    Reload { id: uuid::Uuid, reason: String },
}

#[derive(Clone, From, Deref)]
pub struct AgentCtlSignalSender(Sender<AgentCtlSignal>);
