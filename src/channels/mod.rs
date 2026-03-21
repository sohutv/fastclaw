use crate::agent::{AgentMessageSender, AgentSignal};
use crate::channels::cli_channel::CliChannel;
use crate::channels::dingtalk_channel::DingtalkChannel;
use crate::config::Config;
use derive_more::{Deref, From};
use std::thread::JoinHandle;
use tokio::sync::mpsc::Sender;

#[cfg(feature = "channel_cli_channel")]
pub(crate) mod cli_channel;
mod console_cmd;
#[cfg(feature = "channel_dingtalk_channel")]
pub(crate) mod dingtalk_channel;

pub enum Channel {
    #[cfg(feature = "channel_cli_channel")]
    Cli {
        channel: CliChannel,
        sender: ChannelMessageSender,
    },
    #[cfg(feature = "channel_dingtalk_channel")]
    Dingtalk {
        channel: DingtalkChannel,
        sender: ChannelMessageSender,
    },
}

#[allow(unused)]
#[derive(Clone)]
pub struct ChannelContext {
    pub config: Config,
}

#[derive(Clone, Deref, From)]
pub struct ChannelMessageSender(Sender<AgentSignal>);

impl Channel {
    #[cfg(feature = "channel_cli_channel")]
    pub fn cli_channel(
        config: &'static Config,
        agent_message_sender: AgentMessageSender,
    ) -> crate::Result<Self> {
        let (channel, sender) = CliChannel::new(config, agent_message_sender)?;
        Ok(Self::Cli { channel, sender })
    }

    #[cfg(feature = "channel_dingtalk_channel")]
    pub fn dingtalk_channel(
        config: &'static Config,
        agent_message_sender: AgentMessageSender,
    ) -> crate::Result<Self> {
        let (channel, sender) = DingtalkChannel::new(config, agent_message_sender)?;
        Ok(Self::Dingtalk { channel, sender })
    }
}

impl Channel {
    pub async fn start(self) -> crate::Result<JoinHandle<()>> {
        match self {
            Channel::Cli { channel, .. } => channel.start().await,
            Channel::Dingtalk { channel, .. } => channel.start().await,
        }
    }
    pub fn sender(&self) -> ChannelMessageSender {
        match self {
            Channel::Cli { sender, .. } => sender.clone(),
            Channel::Dingtalk { sender, .. } => sender.clone(),
        }
    }
}
