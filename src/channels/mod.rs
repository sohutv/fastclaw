use crate::agent::{Agent, AgentId, AgentRegistry, AgentRequest, AgentResponse};
use crate::config::{Config, Workspace};
use async_trait::async_trait;
use derive_more::{Deref, From};
use strum::Display;
use tokio::sync::mpsc::{Receiver, Sender};

#[cfg(feature = "channel_cli_channel")]
pub mod cli_channel;
mod console_cmd;
#[cfg(feature = "channel_dingtalk_channel")]
pub mod dingtalk_channel;

#[cfg(feature = "channel_wechat_channel")]
pub mod wechat_channel;

#[cfg(feature = "channel_http_channel")]
pub mod http_channel;

pub mod spawn_agent_request;
pub mod text_formater;

pub mod a2a_channel;
mod session_id;

pub use session_id::*;

#[derive(Clone, From, Deref)]
pub struct ChannelNotifier(Sender<Notify>);
#[derive(Clone)]
pub struct Notify {
    pub agent_id: AgentId,
    pub title: String,
    pub content: String,
}

#[async_trait]
pub trait Channel: Sync + Send
where
    Self: 'static,
{
    type Client: Sync + Send;

    type JoinHandle: Sync + Send;

    async fn start(
        &'static self,
    ) -> crate::Result<(&'static Self, ChannelNotifier, Self::JoinHandle)>;

    fn context(&self) -> &'static ChannelContext;

    async fn client(&self) -> crate::Result<Self::Client>;

    /// handle_agent_task
    /// spawn_agent_task -> spawn(handle_agent_message)
    async fn spawn_agent_request(
        &'static self,
        req: AgentRequest,
    ) -> crate::Result<tokio::task::JoinHandle<()>> {
        let client = self.client().await?;
        let agent = self.context().agent_registry.get(&req.agent_id).await?;
        let mut receiver = spawn_agent_request::apply(&*agent, req).await?;
        let join_handle = tokio::spawn(async move {
            let _ = self.handle_agent_message(&client, &mut receiver).await;
        });
        Ok(join_handle)
    }

    async fn handle_agent_message(
        &self,
        client: &Self::Client,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()>;

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>>;
}

#[allow(unused)]
#[derive(Clone)]
pub struct ChannelContext {
    pub config: &'static Config,
    pub workspace: &'static Workspace,
    pub agent_registry: &'static AgentRegistry,
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

pub async fn create_outbound_msg<Client, P, Content, F, OutboundMsg>(
    client: &Client,
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
    F: FnOnce(
        &Client,
        &dyn Agent,
        &SessionId,
        &ChannelContext,
        Content,
    ) -> crate::Result<OutboundMsg>,
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
    let msg = outbound_msg_creator(client, agent, &session_id, ctx, content)?;
    Ok(Some(msg))
}
