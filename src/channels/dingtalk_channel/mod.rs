use crate::agent::{Agent, MainAgent};
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use anyhow::anyhow;
use async_trait::async_trait;
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
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

mod config;
pub use config::DingTalkConfig;
mod handle_input_message;
mod recv_agent_message;

pub struct DingtalkChannel {
    context: &'static ChannelContext,
    pub dingtalk_config: DingTalkConfig,
    pub dingtalk_client: Arc<RwLock<Option<Arc<DingTalkStream>>>>,
    pub agent: Arc<MainAgent>,
}

impl DingtalkChannel {
    pub async fn new(
        context: &'static ChannelContext,
        agent: &Arc<MainAgent>,
    ) -> crate::Result<Self> {
        Ok(Self {
            context,
            dingtalk_config: context
                .config
                .dingtalk_config
                .clone()
                .ok_or(anyhow!("dingtalk config not found"))?,
            dingtalk_client: Default::default(),
            agent: Arc::clone(agent),
        })
    }
}

#[async_trait]
impl Channel for DingtalkChannel {
    type Client = DingTalkStream;
    type JoinHandle = JoinHandle<crate::Result<()>>;

    async fn start(&'static self) -> crate::Result<(&'static Self, Self::JoinHandle)> {
        let mut guard = self.dingtalk_client.write().await;
        if guard.is_some() {
            return Err(anyhow!("channel had been already started!!!"));
        }
        let cb_handler = Arc::new(handle_input_message::DingTalkCallbackHandler {
            channel: self,
            dingtalk_bot_topic: MessageTopic::Callback(dingtalk_stream::TOPIC_ROBOT.to_string()),
        });
        let (dingtalk, dingtalk_stream_handle) = Arc::new(
            DingTalkStream::new(self.dingtalk_config.credential.clone())
                .register_lifecycle_listener(Arc::clone(&cb_handler))
                .await
                .register_callback_handler(Arc::clone(&cb_handler))
                .await,
        )
        .start()
        .await?;
        *guard = Some(dingtalk);
        Ok((self, dingtalk_stream_handle))
    }

    fn context(&self) -> &'static ChannelContext {
        self.context
    }

    async fn client(&self) -> crate::Result<Arc<Self::Client>> {
        self.dingtalk_client
            .read()
            .await
            .as_ref()
            .map(|it| Arc::clone(it))
            .ok_or(anyhow!("channel not started"))
    }

    async fn handle_agent_message(
        &self,
        dingtalk: Arc<Self::Client>,
        _message_from: Arc<dyn Agent>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message) = receiver.recv().await {
            match message {
                Ok(message) => {
                    match self
                        .handle_agent_message_actual(&dingtalk, &message, state, &mut buff)
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
            .session_configs
            .iter()
            .map(|it| &it.session_id)
            .collect_vec();
        Ok(arr)
    }
}

impl DingtalkChannel {
    fn create_robot_messages<Content: Into<MessageContent>>(
        _: &DingTalkStream,
        _: &dyn Agent,
        session_id: &SessionId,
        _: &ChannelContext,
        content: Content,
    ) -> crate::Result<RobotMessage> {
        Self::create_robot_messages_actual(session_id, content)
    }

    fn create_robot_messages_actual<Content: Into<MessageContent>>(
        session_id: &SessionId,
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
