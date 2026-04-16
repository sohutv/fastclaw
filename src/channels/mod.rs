use crate::agent::{Agent, AgentRequest, AgentResponse};
use crate::config::{Config, Workspace};
use async_trait::async_trait;
use derive_more::Deref;
use log::{error, info};
use std::sync::Arc;
use strum::Display;
use tokio::sync::mpsc::Receiver;

#[cfg(feature = "channel_cli_channel")]
pub mod cli_channel;
mod console_cmd;
#[cfg(feature = "channel_dingtalk_channel")]
pub mod dingtalk_channel;

#[cfg(feature = "channel_wechat_channel")]
pub mod wechat_channel;

pub mod a2a_channel;
mod session_id;
pub use session_id::*;

#[async_trait]
pub trait Channel {
    type Output;

    async fn start(self, agent: Arc<dyn Agent>) -> crate::Result<Self::Output>;
}

async fn spawn_agent_task<F>(
    req: AgentRequest,
    agent_supplier: F,
    addi_system_prompt: Option<String>,
) -> crate::Result<Receiver<ChannelMessage>>
where
    F: FnOnce() -> Arc<dyn Agent>,
{
    let agent = agent_supplier();
    let (channel_message_sender, channel_message_receiver) = tokio::sync::mpsc::channel(32);
    tokio::spawn(async move {
        let task_id = req.id.clone();
        match agent
            .run(
                req,
                channel_message_sender.clone(),
                addi_system_prompt.as_deref(),
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

#[allow(unused)]
#[derive(Clone)]
pub struct ChannelContext {
    pub config: Config,
    pub workspace: &'static Workspace,
}

#[derive(Clone, Deref)]
pub struct ChannelMessage {
    pub session_id: SessionId,
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

async fn create_robot_messages_for_agent<Content, F, OutboundMsg>(
    session_id: &SessionId,
    ctx: &ChannelContext,
    resp_type: AgentRespType,
    content: Content,
    outbound_msg_creator: F,
) -> crate::Result<Option<OutboundMsg>>
where
    F: FnOnce(&SessionId, &ChannelContext, Content) -> crate::Result<OutboundMsg>,
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
    let msg = outbound_msg_creator(&session_id, ctx, content)?;
    Ok(Some(msg))
}
