use crate::agent::ToolFilter;
use crate::channels::{ChannelMessage, SessionId};
use crate::type_::Preamble;
use chrono::Local;
use derive_more::{Deref, Display, From, FromStr, Into};
use rig::OneOrMany;
use rig::message::UserContent;
use tokio::sync::mpsc::Sender;

pub struct AgentRequestPkg {
    pub req: AgentRequest,
    pub ctx: AgentRequestContext,
    pub ack_sender: Option<tokio::sync::oneshot::Sender<()>>,
    #[allow(unused)]
    pub create_time: chrono::DateTime<Local>,
}

impl AgentRequestPkg {
    pub fn new(
        req: AgentRequest,
        ctx: AgentRequestContext,
        ack_sender: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> Self {
        Self {
            req,
            ctx,
            ack_sender,
            create_time: Local::now(),
        }
    }

    pub fn new_with_ack(
        req: AgentRequest,
        ctx: AgentRequestContext,
    ) -> (Self, tokio::sync::oneshot::Receiver<()>) {
        let (ack_sender, ack) = tokio::sync::oneshot::channel();
        (Self::new(req, ctx, Some(ack_sender)), ack)
    }

    pub fn new_without_ack(req: AgentRequest, ctx: AgentRequestContext) -> Self {
        Self::new(req, ctx, None)
    }
}

#[derive(Debug, Clone, Deref, Into)]
pub struct AgentRequest {
    pub id: RequestId,
    pub session_id: SessionId,
    #[deref]
    #[into]
    pub message: Vec<OneOrMany<UserContent>>,
    pub addi_preamble: Option<Preamble>,
}

impl AgentRequest {
    pub fn new<M: Into<UserContent>>(session_id: &SessionId, message: M) -> Self {
        Self {
            id: Default::default(),
            session_id: session_id.clone(),
            message: vec![OneOrMany::one(message.into())],
            addi_preamble: None,
        }
    }
}

#[derive(Clone)]
pub struct AgentRequestContext {
    pub channel_message_sender: Sender<crate::Result<ChannelMessage>>,
    pub tool_filter: ToolFilter,
    pub with_history: bool,
}

#[derive(Debug, Clone, Deref, Display, From, FromStr)]
pub struct RequestId(String);

impl From<uuid::Uuid> for RequestId {
    fn from(value: uuid::Uuid) -> Self {
        value.to_string().into()
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}
