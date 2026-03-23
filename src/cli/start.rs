use crate::cli::CmdRunner;
use crate::config::Config;
use crate::heartbeat::Heartbeat;
use anyhow::anyhow;
use clap::Args;
use derive_more::FromStr;
use log::info;
use std::path::PathBuf;

#[derive(Args)]
pub struct Start {
    #[arg(long)]
    workdir: Option<PathBuf>,
    #[arg(long)]
    channel: Channel,
}

#[derive(Debug, Clone, FromStr)]
pub enum Channel {
    #[cfg(feature = "channel_cli_channel")]
    /// start with cli
    Cli,
    #[cfg(feature = "channel_dingtalk_channel")]
    /// start with dingtalk
    Dingtalk,
}

impl CmdRunner for Start {
    async fn run(&self) -> crate::Result<()> {
        let Self { workdir, channel } = self;
        let workdir = workdir
            .as_deref()
            .map(|it| it.to_owned())
            .unwrap_or_else(|| Config::default_workdir());
        if !workdir.exists() {
            return Err(anyhow!("workdir does not exist: {}", workdir.display()));
        }
        let config = {
            let config_toml = tokio::fs::read_to_string(workdir.join("config.toml")).await?;
            let config = Box::leak(Box::new(toml::from_str::<Config>(&config_toml)?));
            config
        };
        let _ = config.init_logger()?;
        let (main_agent_channel_sender, mut main_agent_channel_message_receiver, main_agent_handle) = {
            let (agent, channel_message_receiver) = crate::agent::create_agent(
                "main",
                config,
                &workdir,
                config.default_model_provider()?,
                config.default_model().clone(),
            )
            .await?;
            let sender = agent.msg_sender();
            let handle = agent.run()?;
            (sender, channel_message_receiver, handle)
        };
        let (
            heartbeat_agent_channel_sender,
            mut heartbeat_agent_channel_message_receiver,
            heartbeat_agent_handle,
        ) = {
            let (agent, channel_message_receiver) = crate::agent::create_agent(
                "heartbeat",
                config,
                &workdir,
                config.default_model_provider()?,
                config.default_model().clone(),
            )
            .await?;
            let sender = agent.msg_sender();
            let handle = agent.run()?;
            (sender, channel_message_receiver, handle)
        };

        let heartbeat_handle = {
            let mut heartbeat = Heartbeat::new(config)?;
            let handle = heartbeat.start(heartbeat_agent_channel_sender).await?;
            handle
        };
        let (channel_message_sender, channel_handle) = match channel {
            #[cfg(feature = "channel_cli_channel")]
            Channel::Cli => {
                info!("Starting CLI channel");
                let cli_channel = crate::channels::Channel::cli_channel(
                    config,
                    main_agent_channel_sender.clone(),
                )?;
                let sender = cli_channel.sender();
                let handle = cli_channel.start().await?;
                (sender, handle)
            }
            #[cfg(feature = "channel_dingtalk_channel")]
            Channel::Dingtalk => {
                info!("Starting Dingtalk channel");
                let dingtalk_channel = crate::channels::Channel::dingtalk_channel(
                    config,
                    main_agent_channel_sender.clone(),
                )?;
                let sender = dingtalk_channel.sender();
                let handle = dingtalk_channel.start().await?;
                (sender, handle)
            }
        };
        loop {
            tokio::select! {
                message = main_agent_channel_message_receiver.recv()=>{
                    if let Some(message) = message{
                        let _ = channel_message_sender.send(message).await;
                    } else {
                        break;
                    }
                },
                message = heartbeat_agent_channel_message_receiver.recv()=> {
                    if let Some(message) = message{
                        let _ = channel_message_sender.send(message).await;
                    } else {
                        break;
                    }
                }
                _= tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        let _ = main_agent_handle.await;
        let _ = heartbeat_agent_handle.await;
        let _ = heartbeat_handle.await;
        let _ = channel_handle.join();
        Ok(())
    }
}
