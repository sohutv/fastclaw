use crate::agent::{Agent, AgentRequest};
use crate::channels::{Channel, ChannelContext, ChannelMessage, SessionId};
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
    dingtalk_config: DingTalkConfig,
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
    type Output = (Arc<DingTalkStream>, JoinHandle<crate::Result<()>>);

    async fn start(self, agent: Arc<dyn Agent>) -> crate::Result<Self::Output> {
        let Self {
            ctx,
            dingtalk_config,
        } = self;
        let cb_handler = Arc::new(callback_handler::DingTalkCallbackHandler {
            ctx: Arc::clone(&ctx),
            config: dingtalk_config.clone(),
            dingtalk_bot_topic: MessageTopic::Callback(dingtalk_stream::TOPIC_ROBOT.to_string()),
            agent: Arc::clone(&agent),
        });
        let (dingtalk, dingtalk_stream_handle) = Arc::new(
            DingTalkStream::new(dingtalk_config.credential)
                .register_lifecycle_listener(Arc::clone(&cb_handler))
                .await
                .register_callback_handler(Arc::clone(&cb_handler))
                .await,
        )
        .start()
        .await?;
        Ok((dingtalk, dingtalk_stream_handle))
    }
}
impl DingtalkChannel {
    pub async fn spawn_agent_task<F>(
        req: AgentRequest,
        agent_supplier: F,
        addi_system_prompt: Option<String>,
    ) -> crate::Result<Receiver<ChannelMessage>>
    where
        F: FnOnce() -> Arc<dyn Agent>,
    {
        super::spawn_agent_task(req, agent_supplier, addi_system_prompt).await
    }
}
impl DingtalkChannel {
    async fn create_robot_messages<Content: Into<MessageContent>>(
        session_id: &SessionId,
        _: &ChannelContext,
        content: Content,
    ) -> Option<RobotMessage> {
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
        Some(message)
    }
}
