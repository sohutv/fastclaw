use crate::agent::{Agent, HistoryManager, JsonlHistoryManager, LlmAgentSupplier};
use crate::channels;
use crate::channels::Channel;
use crate::cli::CmdRunner;
use crate::config::{Config, Workspace};
use crate::heartbeat::Heartbeat;
use crate::model_provider::ModelProviders;
use anyhow::anyhow;
use clap::Args;
use derive_more::FromStr;
use itertools::Itertools;
use log::info;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Args)]
pub struct Start {
    #[arg(long)]
    workdir: Option<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    channel: Vec<ChannelType>,
}

#[derive(Debug, Clone, FromStr, Eq, PartialEq, Ord, PartialOrd, Hash)]
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
        let Self {
            workdir,
            channel: channels,
        } = self;
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
        let (main_agent, heartbeat_agent) = {
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
            let heartbeat_agent = main_agent.fork_with("heartbeat").await?;
            (
                Arc::new(main_agent) as Arc<dyn Agent>,
                Arc::new(heartbeat_agent) as Arc<dyn Agent>,
            )
        };

        enum JoinHandle {
            Std(std::thread::JoinHandle<()>),
            Tokio(tokio::task::JoinHandle<()>),
        }

        let mut join_handles = vec![];
        for channel in channels.into_iter().unique() {
            match channel {
                #[cfg(feature = "channel_cli_channel")]
                ChannelType::Cli => {
                    info!("Starting CLI channel");
                    let channel = channels::cli_channel::CliChannel::new(config, workspace).await?;
                    let join_handle = channel.start(Arc::clone(&main_agent)).await?;
                    join_handles.push(JoinHandle::Std(join_handle));
                }
                #[cfg(feature = "channel_dingtalk_channel")]
                ChannelType::Dingtalk => {
                    let join_handle = start_with_dingtalk(
                        config,
                        workspace,
                        Arc::clone(&history_manager),
                        Arc::clone(&main_agent),
                        Arc::clone(&heartbeat_agent),
                    )
                    .await?;
                    join_handles.push(JoinHandle::Tokio(join_handle));
                }
                #[cfg(feature = "channel_wechat_channel")]
                ChannelType::Wechat => {
                    let join_handle = start_with_wechat(
                        config,
                        workspace,
                        Arc::clone(&history_manager),
                        Arc::clone(&main_agent),
                        Arc::clone(&heartbeat_agent),
                    )
                    .await?;
                    join_handles.push(JoinHandle::Tokio(join_handle));
                }
            }
        }
        for join_handle in join_handles {
            match join_handle {
                JoinHandle::Std(it) => {
                    let _ = it.join();
                }
                JoinHandle::Tokio(it) => {
                    let _ = it.await;
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "channel_dingtalk_channel")]
async fn start_with_dingtalk(
    config: &'static Config,
    workspace: &'static Workspace,
    history_manager: Arc<RwLock<dyn HistoryManager>>,
    main_agent: Arc<dyn Agent>,
    heartbeat_agent: Arc<dyn Agent>,
) -> crate::Result<tokio::task::JoinHandle<()>> {
    info!("Starting Dingtalk channel");
    let channel = channels::dingtalk_channel::DingtalkChannel::new(config, workspace).await?;
    let session_ids = channel
        .dingtalk_config
        .allow_session_ids
        .values()
        .into_iter()
        .map(|it| it.clone())
        .collect_vec();
    let channel_ctx = Arc::clone(&(channel.ctx));
    let (dingtalk, chanel_join_handle) = channel.start(main_agent).await?;
    let heartbeat_join_handle = {
        let channel_ctx = Arc::clone(&channel_ctx);
        let dingtalk = Arc::clone(&dingtalk);
        let heartbeat = Heartbeat::new(config, workspace, &history_manager).await?;

        let join_handle = heartbeat
            .start(session_ids, heartbeat_agent, move |agent, req| {
                let channel_ctx = Arc::clone(&channel_ctx);
                let dingtalk = Arc::clone(&dingtalk);
                async move {
                    let mut receiver =
                        channels::dingtalk_channel::DingtalkChannel::spawn_agent_task(
                            req,
                            || agent,
                            None,
                        )
                        .await?;
                    let _ = channels::dingtalk_channel::DingtalkChannel::recv_agent_message(
                        dingtalk,
                        &channel_ctx,
                        &mut receiver,
                    )
                    .await;
                    Ok(())
                }
            })
            .await?;
        join_handle
    };
    let join_handle = tokio::spawn(async {
        let _ = chanel_join_handle.await;
        let _ = heartbeat_join_handle.await;
    });
    Ok(join_handle)
}

#[cfg(feature = "channel_wechat_channel")]
async fn start_with_wechat(
    config: &'static Config,
    workspace: &'static Workspace,
    history_manager: Arc<RwLock<dyn HistoryManager>>,
    main_agent: Arc<dyn Agent>,
    heartbeat_agent: Arc<dyn Agent>,
) -> crate::Result<tokio::task::JoinHandle<()>> {
    info!("Starting Wechat channel");
    let channel = channels::wechat_channel::WechatChannel::new(config, workspace).await?;
    let session_ids = vec![channel.wechat_config.session_id.clone()];
    let channel_ctx = Arc::clone(&(channel.ctx));
    let (wechat, chanel_join_handle) = channel.start(main_agent).await?;
    let heartbeat_join_handle = {
        let channel_ctx = Arc::clone(&channel_ctx);
        let wechat = Arc::clone(&wechat);
        let heartbeat = Heartbeat::new(config, workspace, &history_manager).await?;
        let join_handle = heartbeat
            .start(session_ids, heartbeat_agent, move |agent, req| {
                let channel_ctx = Arc::clone(&channel_ctx);
                let wechat = Arc::clone(&wechat);
                async move {
                    let mut receiver = channels::wechat_channel::WechatChannel::spawn_agent_task(
                        req,
                        || agent,
                        None,
                    )
                    .await?;
                    let _ = channels::wechat_channel::WechatChannel::recv_agent_message(
                        wechat,
                        &channel_ctx,
                        &mut receiver,
                    )
                    .await;
                    Ok(())
                }
            })
            .await?;
        join_handle
    };
    let join_handle = tokio::spawn(async {
        let _ = chanel_join_handle.await;
        let _ = heartbeat_join_handle.await;
    });
    Ok(join_handle)
}
