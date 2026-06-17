use crate::agent::{Agent, AgentId, AgentRequest, AgentResponse};
use crate::config::{Config, Workspace};
use async_trait::async_trait;
use derive_more::Deref;
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

#[cfg(feature = "channel_http_channel")]
pub mod http_channel;

mod spawn_agent_task;
pub mod text_formater;

mod session_id;
pub use session_id::*;

#[async_trait]
pub trait Channel: Sync + Send
where
    Self: 'static,
{
    type Client: Sync + Send;

    type JoinHandle: Sync + Send;

    async fn start(self) -> crate::Result<(Arc<Self>, Arc<Self::Client>, Self::JoinHandle)>;

    fn agent(&self) -> &Arc<dyn Agent>;

    /// handle_agent_task
    /// spawn_agent_task -> spawn(handle_agent_message)
    async fn append_agent_task(
        self: Arc<Self>,
        client: Arc<Self::Client>,
        addi_system_prompt: Option<String>,
        req: AgentRequest,
    ) -> crate::Result<()> {
        let mut receiver =
            spawn_agent_task::apply(Arc::clone(self.agent()), req.clone(), addi_system_prompt)
                .await?;
        let self_ = Arc::clone(&self);
        let _ = tokio::spawn(async move {
            let _ = self_.handle_agent_message(client, &mut receiver).await;
        });
        Ok(())
    }

    async fn handle_agent_message(
        &self,
        client: Arc<Self::Client>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()>;

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>>;
}

#[allow(unused)]
#[derive(Clone)]
pub struct ChannelContext {
    pub config: &'static Config,
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
    ToolResult,
    Reasoning,
    Content,
    Notify,
    HistoryCompactOk,
    HistoryCompactErr,
    HistoryCompactIgnore,
    Error,
}

#[derive(Debug, Copy, Clone, Display)]
pub enum AgentRespState {
    Wait,
    Start,
    Reasoning,
    Messaging,
    Final,
}

pub async fn create_robot_messages_for_agent<P, Content, F, OutboundMsg>(
    agent: &dyn Agent,
    session_id: &SessionId,
    session_settings_provider: &P,
    ctx: &ChannelContext,
    resp_type: AgentRespType,
    content: Content,
    outbound_msg_creator: F,
) -> crate::Result<Option<OutboundMsg>>
where
    P: SessionSettingsProvider,
    F: FnOnce(&dyn Agent, &SessionId, &ChannelContext, Content) -> crate::Result<OutboundMsg>,
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
    } = session_id.settings(session_settings_provider)?;
    match resp_type {
        AgentRespType::Start => {
            let true = show_start else {
                return Ok(None);
            };
        }
        AgentRespType::ToolCall | AgentRespType::ToolResult => {
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
    let msg = outbound_msg_creator(agent, &session_id, ctx, content)?;
    Ok(Some(msg))
}
