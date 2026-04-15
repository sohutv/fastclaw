use crate::agent::{HistoryManager, JsonlHistoryManager, LlmAgentSupplier};
use crate::channels;
use crate::channels::Channel;
use crate::cli::CmdRunner;
use crate::config::{Config, Workspace};
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
    #[cfg(feature = "channel_wechat_channel")]
    /// start with wechat
    Wechat,
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
        let workspace = { Box::leak(Box::new(Workspace::init(workdir).await?)) };
        let history_manager: Arc<RwLock<dyn HistoryManager>> = {
            let mgr = JsonlHistoryManager::new(workspace).await?;
            Arc::new(RwLock::new(mgr))
        };
        let main_agent = match config.default_model_provider()? {
            ModelProviders::OpenaiCompatible(model_provider) => {
                model_provider
                    .create_agent(
                        "main",
                        config,
                        config.default_model().clone(),
                        Some(Arc::clone(&history_manager)),
                        workspace,
                    )
                    .await?
            }
        };
        match channel {
            #[cfg(feature = "channel_cli_channel")]
            ChannelType::Cli => {
                info!("Starting CLI channel");
                let channel = channels::cli_channel::CliChannel::new(config, workspace).await?;
                let _ = channel.start(Arc::new(main_agent)).await?.join();
            }
            #[cfg(feature = "channel_dingtalk_channel")]
            ChannelType::Dingtalk => {
                info!("Starting Dingtalk channel");
                let channel =
                    channels::dingtalk_channel::DingtalkChannel::new(config, workspace).await?;
                let channel_ctx = Arc::clone(&(channel.ctx));
                let heartbeat_agent = Arc::new(main_agent.fork("heartbeat").await?);
                let main_agent = Arc::new(main_agent);
                let (dingtalk, chanel_join_handle) = channel.start(main_agent).await?;
                let heartbeat_join_handle = {
                    let channel_ctx = Arc::clone(&channel_ctx);
                    let dingtalk_client = Arc::clone(&dingtalk);
                    let heartbeat = Heartbeat::new(config, workspace, &history_manager).await?;
                    let join_handle = heartbeat.start(
                        heartbeat_agent,
                        move|agent, req| {
                            let channel_ctx = Arc::clone(&channel_ctx);
                            let dingtalk_client = Arc::clone(&dingtalk_client);
                            async move {
                                let mut receiver = channels::dingtalk_channel::DingtalkChannel::spawn_agent_task(req, || agent, None).await?;
                                let _ = channels::dingtalk_channel::DingtalkChannel::recv_agent_message(dingtalk_client, &channel_ctx, &mut receiver).await;
                                Ok(())
                            }
                        },
                    ).await?;
                    join_handle
                };
                let _ = chanel_join_handle.await;
                let _ = heartbeat_join_handle.await;
            }
            #[cfg(feature = "channel_wechat_channel")]
            ChannelType::Wechat => {
                info!("Starting Wechat channel");
                let channel =
                    channels::wechat_channel::WechatChannel::new(config, workspace).await?;
                let channel_ctx = Arc::clone(&(channel.ctx));
                let main_agent = Arc::new(main_agent);
                let (wechat, chanel_join_handle) = channel.start(main_agent).await?;
                let _ = chanel_join_handle.await;
            }
        };
        Ok(())
    }
}
