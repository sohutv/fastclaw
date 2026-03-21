use crate::cli::CmdRunner;
use crate::config::Config;
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
        let (message_sender, mut message_receiver) = tokio::sync::mpsc::channel(1024);
        let (main_agent_channel_sender, main_agent_handle) = {
            let main_agent = crate::agent::create_agent(
                "main",
                config,
                &workdir,
                config.default_model_provider()?,
                config.default_model().clone(),
                message_sender.into(),
            )
            .await?;
            let sender = main_agent.msg_sender();
            let handle = main_agent.run()?;
            (sender, handle)
        };

        let (channel_message_sender, channel_handle) = match channel {
            #[cfg(feature = "channel_cli_channel")]
            Channel::Cli => {
                info!("Starting CLI channel");
                let cli_channel =
                    crate::channels::Channel::cli_channel(config, main_agent_channel_sender.clone())?;
                let sender = cli_channel.sender();
                let handle = cli_channel.start().await?;
                (sender, handle)
            }
        };
        loop {
            tokio::select! {
                message = message_receiver.recv()=>{
                    if let Some(message)= message{
                        let _ = channel_message_sender.send(message).await;
                    }
                },
                _= tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        let _ = main_agent_handle.await;
        let _ = channel_handle.join();
        Ok(())
    }
}
