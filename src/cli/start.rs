use crate::agent::{Agent, HistoryManager, JsonlHistoryManager, LlmAgentSupplier, Workspace};
use crate::channels;
use crate::channels::{Channel, ChannelMessage};
use crate::cli::CmdRunner;
use crate::config::Config;
use crate::heartbeat::Heartbeat;
use crate::model_provider::ModelProviders;
use anyhow::anyhow;
use clap::Args;
use derive_more::FromStr;
use log::info;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Args)]
pub struct Start {
    #[arg(long)]
    workdir: Option<PathBuf>,
    #[arg(long)]
    channel: ChannelType,
}

#[derive(Debug, Clone, FromStr)]
pub enum ChannelType {
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
        let workspace = { Box::leak(Box::new(Workspace::from(workdir))) };
        let history_manager: Arc<RwLock<dyn HistoryManager>> = {
            let mgr = JsonlHistoryManager::new(workspace).await?;
            Arc::new(RwLock::new(mgr))
        };
        let (channel_message_sender, channel_message_receiver) =
            tokio::sync::mpsc::channel::<ChannelMessage>(1024);
        let (main_agent_handle, main_agent_message_sender) = {
            let agent = match config.default_model_provider()? {
                ModelProviders::OpenaiCompatible(model_provider) => {
                    model_provider
                        .create_agent(
                            "main",
                            config,
                            config.default_model().clone(),
                            &history_manager,
                            workspace,
                        )
                        .await?
                }
            };
            agent.run(channel_message_sender.clone())?
        };

        let (heartbeat_agent_handle, heartbeat_agent_message_sender) = {
            let agent = Box::new(match config.default_model_provider()? {
                ModelProviders::OpenaiCompatible(model_provider) => {
                    model_provider
                        .create_agent(
                            "heartbeat",
                            config,
                            config.default_model().clone(),
                            &history_manager,
                            workspace,
                        )
                        .await?
                }
            });
            agent.run(channel_message_sender.clone())?
        };
        let heartbeat_handle = {
            let mut heartbeat = Heartbeat::new(config, &history_manager, workspace).await?;
            let handle = heartbeat.start(heartbeat_agent_message_sender).await?;
            handle
        };
        let channel_handle = match channel {
            #[cfg(feature = "channel_cli_channel")]
            ChannelType::Cli => {
                info!("Starting CLI channel");
                let cli_channel =
                    channels::cli_channel::CliChannel::new(config, main_agent_message_sender)?;
                cli_channel.start(channel_message_receiver).await?
            }
            #[cfg(feature = "channel_dingtalk_channel")]
            ChannelType::Dingtalk => {
                info!("Starting Dingtalk channel");
                let dingtalk_channel = channels::dingtalk_channel::DingtalkChannel::new(
                    config,
                    main_agent_message_sender,
                )?;
                dingtalk_channel.start(channel_message_receiver).await?
            }
        };
        let _ = main_agent_handle.await;
        let _ = heartbeat_agent_handle.await;
        let _ = heartbeat_handle.await;
        let _ = channel_handle.join();
        Ok(())
    }
}
