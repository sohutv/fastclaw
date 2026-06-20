use crate::agent::{
    Agent, AgentContext, AgentGroup, AgentId, AgentRegistry, HistoryManager, JsonlHistoryManager,
    MainAgent, OwnerSession,
};
use crate::channels::{Channel, ChannelContext, ChannelNotifier};
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
use std::ops::DerefMut;
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
            if *verbose {
                log_config = log_config.update_level(Level::Debug);
            }
            log_config.init(&workdir)?;
        }

        let mcp_registry = Box::leak(Box::new(McpRegistry::new(config)?.init().await?));
        let workspace = { Box::leak(Box::new(Workspace::init(workdir).await?)) };
        let history_manager: Arc<dyn HistoryManager> =
            Arc::new(JsonlHistoryManager::new(config, workspace).await?);
        let memory_manager = Arc::new(MemoryManager::new(config, workspace).await?);
        let agent_registry = Box::leak(Box::new(AgentRegistry::new(config, workspace)?));
        let channel_context = Box::leak(Box::new(ChannelContext {
            config,
            workspace,
            agent_registry,
        }));
        let (a2a_channel, ..) = Box::leak(Box::new(
            channels::a2a_channel::A2AChannel::new(channel_context).await?,
        ))
        .start()
        .await?;
        let channel_notifier = Default::default();
        let agent_context = Box::leak(Box::new(AgentContext {
            config,
            workspace,
            history_manager,
            memory_manager,
            mcp_registry,
            agent_registry,
            a2a_channel,
            channel_notifier: Arc::clone(&channel_notifier),
        }));
        let (main_agent, heartbeat_agent) = {
            let main_agent = {
                let (agent_id, agent_group) = AgentId::main();
                Arc::new(
                    MainAgent::new(
                        agent_registry
                            .get_with(
                                agent_context,
                                &agent_id,
                                |agent_context, agent_id| async move {
                                    agent::spawn_agent(
                                        &agent_id,
                                        &agent_group,
                                        None,
                                        None,
                                        &OwnerSession::GlobalShare,
                                        agent_context,
                                    )
                                    .await
                                },
                            )
                            .await?,
                    )
                    .await?
                    .init_children()
                    .await?,
                )
            };
            let heartbeat_agent = {
                let (agent_id, agent_group) = ("heartbeat".into(), AgentGroup::main());
                agent::spawn_agent(
                    &agent_id,
                    &agent_group,
                    None,
                    None,
                    &OwnerSession::GlobalShare,
                    agent_context,
                )
                .await?
            };
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
                    let (_channel, notifier, join_handle) = Box::leak(Box::new(
                        channels::cli_channel::CliChannel::new(channel_context, &main_agent)
                            .await?,
                    ))
                    .start()
                    .await?;
                    let _ = channel_notifier.write().await.deref_mut().push(notifier);
                    join_handles.push(JoinHandle::Std(join_handle));
                }
                #[cfg(feature = "channel_dingtalk_channel")]
                ChannelType::Dingtalk => {
                    info!("Starting Dingtalk channel");
                    let channel = Box::leak(Box::new(
                        channels::dingtalk_channel::DingtalkChannel::new(
                            channel_context,
                            &main_agent,
                        )
                        .await?,
                    ));
                    let (join_handle, notifier) =
                        bind_channel(config, workspace, channel, Arc::clone(&heartbeat_agent))
                            .await?;
                    let _ = channel_notifier.write().await.deref_mut().push(notifier);
                    join_handles.push(JoinHandle::Tokio(join_handle));
                }
                #[cfg(feature = "channel_wechat_channel")]
                ChannelType::Wechat => {
                    let channel = Box::leak(Box::new(
                        channels::wechat_channel::WechatChannel::new(channel_context, &main_agent)
                            .await?,
                    ));
                    let (join_handle, notifier) =
                        bind_channel(config, workspace, channel, Arc::clone(&heartbeat_agent))
                            .await?;
                    let _ = channel_notifier.write().await.deref_mut().push(notifier);
                    join_handles.push(JoinHandle::Tokio(join_handle));
                }
                #[cfg(feature = "channel_http_channel")]
                ChannelType::Http => {
                    info!("Starting HttpStreamable channel");
                    let channel = Box::leak(Box::new(
                        channels::http_channel::HttpChannel::new(channel_context, &main_agent)
                            .await?,
                    ));
                    let (join_handle, notifier) =
                        bind_channel(config, workspace, channel, Arc::clone(&heartbeat_agent))
                            .await?;
                    let _ = channel_notifier.write().await.deref_mut().push(notifier);
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

async fn bind_channel<C>(
    config: &'static Config,
    workspace: &'static Workspace,
    channel: &'static C,
    heartbeat_agent: Arc<dyn Agent>,
) -> crate::Result<(tokio::task::JoinHandle<()>, ChannelNotifier)>
where
    C: Channel,
    <C as Channel>::JoinHandle: Future + Sync + Send,
{
    let (channel, notifier, chanel_join_handle) = channel.start().await?;
    let (_, heartbeat_join_handle) = Heartbeat::new(config, workspace, channel, heartbeat_agent)?
        .start()
        .await?;
    let join_handle = tokio::spawn(async {
        let _ = chanel_join_handle.await;
        let _ = heartbeat_join_handle.await;
    });
    Ok((join_handle, notifier))
}
