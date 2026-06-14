use crate::agent::{Agent, AgentId, HistoryManager, JsonlHistoryManager};
use crate::channels::Channel;
use crate::cli::CmdRunner;
use crate::config::logger::{Level, Logger};
use crate::config::{Config, Workspace};
use crate::heartbeat::Heartbeat;
use crate::memory::MemoryManager;
use crate::tools::mcp_tool::McpRegistry;
use crate::{agent, channels};
use anyhow::anyhow;
use clap::Args;
use derive_more::FromStr;
use itertools::Itertools;
use log::info;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Args)]
pub struct Start {
    #[arg(long)]
    workdir: Option<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    channel: Vec<ChannelType>,
    #[arg(long, default_value = "false")]
    std_log: bool,
    #[arg(long, default_value = "false")]
    verbose: bool,
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
    #[cfg(feature = "channel_http_channel")]
    /// start with http_channel
    Http,
}

impl CmdRunner for Start {
    async fn run(&self) -> crate::Result<()> {
        let Self {
            workdir,
            channel: channels,
            std_log,
            verbose,
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

        {
            let mut log_config = config.log_config.clone();
            if *std_log {
                log_config = log_config.update_logger(Logger::Stdout);
            }
            if *verbose  {
                log_config = log_config.update_level(Level::Debug);
            }
            log_config.init(&workdir)?;
        }

        let mcp_registry = Box::leak(Box::new(McpRegistry::new(config)?.init().await?));
        let workspace = { Box::leak(Box::new(Workspace::init(workdir).await?)) };
        let history_manager: Arc<dyn HistoryManager> =
            Arc::new(JsonlHistoryManager::new(config, workspace).await?);
        let memory_manager = Arc::new(MemoryManager::new(config, workspace).await?);
        let (main_agent, heartbeat_agent) = {
            let agent_id = AgentId::from("main");
            let main_agent = agent::spawn_agent(
                &agent_id,
                &agent_id.deref().into(),
                config,
                &history_manager,
                &memory_manager,
                workspace,
                mcp_registry,
                |sp| async { Ok(sp) },
            )
            .await?;
            let heartbeat_agent = main_agent.clone_with("heartbeat".into(), None).await?;
            (main_agent, heartbeat_agent)
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
                    let (_, _, join_handle) = channel.start(Arc::clone(&main_agent)).await?;
                    join_handles.push(JoinHandle::Std(join_handle));
                }
                #[cfg(feature = "channel_dingtalk_channel")]
                ChannelType::Dingtalk => {
                    info!("Starting Dingtalk channel");
                    let channel =
                        channels::dingtalk_channel::DingtalkChannel::new(config, workspace).await?;
                    let join_handle = start_channel(
                        config,
                        workspace,
                        channel,
                        Arc::clone(&main_agent),
                        Arc::clone(&heartbeat_agent),
                    )
                    .await?;
                    join_handles.push(JoinHandle::Tokio(join_handle));
                }
                #[cfg(feature = "channel_wechat_channel")]
                ChannelType::Wechat => {
                    let channel =
                        channels::wechat_channel::WechatChannel::new(config, workspace).await?;
                    let join_handle = start_channel(
                        config,
                        workspace,
                        channel,
                        Arc::clone(&main_agent),
                        Arc::clone(&heartbeat_agent),
                    )
                    .await?;
                    join_handles.push(JoinHandle::Tokio(join_handle));
                }
                #[cfg(feature = "channel_http_channel")]
                ChannelType::Http => {
                    info!("Starting HttpStreamable channel");
                    let channel =
                        channels::http_channel::HttpChannel::new(config, workspace).await?;
                    let join_handle = start_channel(
                        config,
                        workspace,
                        channel,
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

async fn start_channel<C>(
    config: &'static Config,
    workspace: &'static Workspace,
    channel: C,
    main_agent: Arc<dyn Agent>,
    heartbeat_agent: Arc<dyn Agent>,
) -> crate::Result<tokio::task::JoinHandle<()>>
where
    C: Channel,
    <C as Channel>::JoinHandle: Future + Sync + Send,
{
    let (channel, client, chanel_join_handle) = channel.start(main_agent).await?;
    let (_, heartbeat_join_handle) = Heartbeat::new(
        config,
        workspace,
        Arc::clone(&channel),
        Arc::clone(&client),
        heartbeat_agent,
    )?
    .start()
    .await?;
    let join_handle = tokio::spawn(async {
        let _ = chanel_join_handle.await;
        let _ = heartbeat_join_handle.await;
    });
    Ok(join_handle)
}
