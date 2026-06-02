use crate::agent::{Agent, AgentId, AgentRequest, AgentRequestContext, AgentResponse};
use crate::config::{Config, Workspace};
use async_trait::async_trait;
use derive_more::Deref;
use log::{error, info};
use std::sync::Arc;
use strum::Display;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

#[cfg(feature = "channel_cli_channel")]
pub mod cli_channel;
mod console_cmd;
#[cfg(feature = "channel_dingtalk_channel")]
pub mod dingtalk_channel;

#[cfg(feature = "channel_wechat_channel")]
pub mod wechat_channel;

#[cfg(feature = "channel_http_channel")]
pub mod http_channel;

pub mod a2a_channel;
mod session_id;
pub use session_id::*;

#[async_trait]
pub trait Channel: Sync + Send
where
    Self: 'static,
{
    type Client: Sync + Send;

    type InboundMessage: Sync + Send;

    type JoinHandle: Sync + Send;

    async fn start(
        self,
        agent: Arc<dyn Agent>,
    ) -> crate::Result<(Arc<Self>, Arc<Self::Client>, Self::JoinHandle)>;

    async fn spawn_agent_task(
        &self,
        agent: Arc<dyn Agent>,
        req: AgentRequest,
        addi_system_prompt: Option<String>,
    ) -> crate::Result<Receiver<crate::Result<ChannelMessage>>> {
        let (channel_message_sender, channel_message_receiver) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            async fn spawn_agent_task_inner(
                agent: Arc<dyn Agent>,
                req: AgentRequest,
                ctx: AgentRequestContext,
            ) -> crate::Result<()> {
                let sender = agent.get_channel_sender().await?;
                let _ = sender.send((req, ctx)).await?;
                Ok(())
            }
            let task_id = req.id.clone();
            match spawn_agent_task_inner(
                agent,
                req,
                AgentRequestContext {
                    channel_message_sender: channel_message_sender,
                    addi_system_prompt,
                    tool_filter: Default::default(),
                    with_history: true,
                },
            )
            .await
            {
                Ok(_) => {
                    info!("Agent run completed, task_id: {}", task_id);
                }
                Err(err) => {
                    error!("Agent run failed, task_id: {}, error: {}", task_id, err);
                }
            }
        });
        Ok(channel_message_receiver)
    }

    async fn handle_agent_message(
        &self,
        client: Arc<Self::Client>,
        inbound_message: Option<Self::InboundMessage>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()>;

    /// handle_agent_task
    /// spawn_agent_task -> spawn(handle_agent_message)
    async fn submit_agent_task(
        self: Arc<Self>,
        client: Arc<Self::Client>,
        agent: Arc<dyn Agent>,
        addi_system_prompt: Option<String>,
        inbound_message: Option<Self::InboundMessage>,
        req: AgentRequest,
    ) -> crate::Result<JoinHandle<crate::Result<()>>> {
        let mut receiver = Arc::clone(&self)
            .spawn_agent_task(agent, req.clone(), addi_system_prompt)
            .await?;
        let self_ = Arc::clone(&self);
        let join_handle = tokio::spawn(async move {
            let _ = self_.handle_agent_message(client, inbound_message, &mut receiver).await?;
            Ok(())
        });
        Ok(join_handle)
    }

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>> {
        Ok(vec![])
    }
}

#[allow(unused)]
#[derive(Clone)]
pub struct ChannelContext {
    pub config: Config,
    pub workspace: &'static Workspace,
}

#[derive(Clone, Deref)]
pub struct ChannelMessage {
    pub session_id: SessionId,
    pub agent_id: AgentId,
    #[deref]
    pub message: AgentResponse,
}

#[derive(Debug, Copy, Clone, Display)]
pub enum AgentRespType {
    Start,
    ToolCall,
    Reasoning,
    Content,
    Notify,
    HistoryCompactOk,
    HistoryCompactErr,
    HistoryCompactIgnore,
    Error,
}

#[derive(Debug, Copy, Clone, Display)]
enum AgentRespState {
    Wait,
    Start,
    Reasoning,
    Messaging,
    Final,
}

async fn create_robot_messages_for_agent<Content, F, InboundMsg, OutboundMsg>(
    session_id: &SessionId,
    ctx: &ChannelContext,
    resp_type: AgentRespType,
    inbound_msg: Option<&InboundMsg>,
    content: Content,
    outbound_msg_creator: F,
) -> crate::Result<Option<OutboundMsg>>
where
    F: FnOnce(&SessionId, &ChannelContext, Option<&InboundMsg>, Content) -> crate::Result<OutboundMsg>,
{
    let SessionSettings {
        show_start,
        show_toolcall,
        show_reasoning,
        show_notify,
        show_compacting,
        show_compacting_ok,
        show_compacting_err,
        show_compacting_ignore,
        show_error,
        ..
    } = session_id.settings();
    match resp_type {
        AgentRespType::Start => {
            let true = show_start else {
                return Ok(None);
            };
        }
        AgentRespType::ToolCall => {
            let true = show_toolcall else {
                return Ok(None);
            };
        }
        AgentRespType::Reasoning => {
            let true = show_reasoning else {
                return Ok(None);
            };
        }
        AgentRespType::Content => {}
        AgentRespType::Notify => {
            let true = show_notify else {
                return Ok(None);
            };
        }
        AgentRespType::HistoryCompactOk => {
            let true = (*show_compacting && *show_compacting_ok) else {
                return Ok(None);
            };
        }
        AgentRespType::HistoryCompactErr => {
            let true = (*show_compacting && *show_compacting_err) else {
                return Ok(None);
            };
        }
        AgentRespType::HistoryCompactIgnore => {
            let true = (*show_compacting && *show_compacting_ignore) else {
                return Ok(None);
            };
        }
        AgentRespType::Error => {
            let true = show_error else {
                return Ok(None);
            };
        }
    }
    let msg = outbound_msg_creator(&session_id, ctx, inbound_msg, content)?;
    Ok(Some(msg))
}
