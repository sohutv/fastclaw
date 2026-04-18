use crate::agent::Agent;
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
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
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;

mod callback_handler;
mod config;
mod recv_agent_message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingTalkConfig {
    pub credential: dingtalk_stream::Credential,
    pub allow_session_ids: BTreeMap<String, SessionId>,
}

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
    type JoinHandle = JoinHandle<crate::Result<()>>;

    async fn start(
        self,
        agent: Arc<dyn Agent>,
    ) -> crate::Result<(Arc<Self>, Arc<Self::Client>, Self::JoinHandle)> {
        let self_ = Arc::new(self);
        let cb_handler = Arc::new(callback_handler::DingTalkCallbackHandler {
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

    async fn recv_agent_message(
        &self,
        dingtalk: Arc<DingTalkStream>,
        receiver: &mut Receiver<ChannelMessage>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message) = receiver.recv().await {
            match self
                .handle_agent_message(&dingtalk, &message, state, &mut buff)
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
        Ok(())
    }
}

impl DingtalkChannel {
    fn create_robot_messages<Content: Into<MessageContent>>(
        session_id: &SessionId,
        _: &ChannelContext,
        content: Content,
    ) -> crate::Result<RobotMessage> {
        let content = content.into();
        let message = match &session_id {
            SessionId::Master { .. } | SessionId::Anonymous { .. } => RobotPrivateMessage {
                user_ids: vec![DingTalkUserId::from(session_id.deref())],
                content: content.clone(),
            }
            .into(),
            SessionId::Group { val: group, .. } => RobotGroupMessage {
                group_id: DingTalkGroupConversationId::from(&group.id),
                content: content.clone(),
            }
            .into(),
        };
        Ok(message)
    }
}
