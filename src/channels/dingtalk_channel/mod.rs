use crate::agent::Agent;
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use dingtalk_stream::frames::down_message::callback_message::MessageData;
use dingtalk_stream::{
    DingTalkStream,
    frames::{
        DingTalkGroupConversationId, DingTalkUserId,
        down_message::MessageTopic,
        up_message::{
            MessageContent,
            robot_message::{RobotGroupMessage, RobotMessage, RobotPrivateMessage},
        },
    },
};
use itertools::Itertools;
use log::warn;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

mod config;
pub use config::DingTalkConfig;
mod handle_input_message;
mod recv_agent_message;

pub struct DingtalkChannel {
    pub ctx: Arc<ChannelContext>,
    pub dingtalk_config: DingTalkConfig,
}

impl DingtalkChannel {
    pub async fn new(
        config: &'static Config,
        workspace: &'static Workspace,
    ) -> crate::Result<Self> {
        Ok(Self {
            ctx: Arc::new(ChannelContext {
                config: config.clone(),
                workspace,
            }),
            dingtalk_config: config
                .dingtalk_config
                .clone()
                .ok_or(anyhow!("dingtalk config not found"))?,
        })
    }
}

#[async_trait]
impl Channel for DingtalkChannel {
    type Client = DingTalkStream;
    type InboundMessage = MessageData;
    type JoinHandle = JoinHandle<crate::Result<()>>;

    async fn start(
        self,
        agent: Arc<dyn Agent>,
    ) -> crate::Result<(Arc<Self>, Arc<Self::Client>, Self::JoinHandle)> {
        let self_ = Arc::new(self);
        let _ = Agent::start(Arc::clone(&agent)).await?;
        let cb_handler = Arc::new(handle_input_message::DingTalkCallbackHandler {
            channel: Arc::clone(&self_),
            dingtalk_bot_topic: MessageTopic::Callback(dingtalk_stream::TOPIC_ROBOT.to_string()),
            agent: Arc::clone(&agent),
        });
        let (dingtalk, dingtalk_stream_handle) = Arc::new(
            DingTalkStream::new(self_.dingtalk_config.credential.clone())
                .register_lifecycle_listener(Arc::clone(&cb_handler))
                .await
                .register_callback_handler(Arc::clone(&cb_handler))
                .await,
        )
        .start()
        .await?;
        Ok((self_, dingtalk, dingtalk_stream_handle))
    }

    async fn handle_agent_message(
        &self,
        dingtalk: Arc<DingTalkStream>,
        agent: Arc<dyn Agent>,
        inbound_message: Option<Self::InboundMessage>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message) = receiver.recv().await {
            match message {
                Ok(message) => {
                    match self
                        .handle_agent_message_actual(
                            &dingtalk,
                            &*agent,
                            inbound_message.as_ref(),
                            &message,
                            state,
                            &mut buff,
                        )
                        .await
                    {
                        Ok(AgentRespState::Final) | Err(_) => {
                            state = AgentRespState::Wait;
                            buff.clear();
                        }
                        Ok(next) => {
                            state = next;
                        }
                    }
                }
                Err(err) => {
                    warn!("recv error channel message: {err}");
                }
            }
        }
        Ok(())
    }

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>> {
        let arr = self
            .dingtalk_config
            .allow_session_ids
            .iter()
            .map(|it| &it.session_id)
            .collect_vec();
        Ok(arr)
    }
}

impl DingtalkChannel {
    fn create_robot_messages<Content: Into<MessageContent>>(
        _: &dyn Agent,
        session_id: &SessionId,
        ctx: &ChannelContext,
        data: Option<&MessageData>,
        content: Content,
    ) -> crate::Result<RobotMessage> {
        Self::create_robot_messages_actual(session_id, ctx, data, content)
    }

    fn create_robot_messages_actual<Content: Into<MessageContent>>(
        session_id: &SessionId,
        _: &ChannelContext,
        _: Option<&MessageData>,
        content: Content,
    ) -> crate::Result<RobotMessage> {
        let content = content.into();
        let message = match &session_id {
            SessionId::Master(_) | SessionId::Anonymous(_) => RobotPrivateMessage {
                user_ids: vec![DingTalkUserId::from(session_id.deref())],
                content: content.clone(),
            }
            .into(),
            SessionId::Group(group) => RobotGroupMessage {
                group_id: DingTalkGroupConversationId::from(&group.id),
                content: content.clone(),
            }
            .into(),
        };
        Ok(message)
    }
}
