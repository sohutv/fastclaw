use crate::agent::{Agent, AgentRequest};
use crate::channels::{Channel, ChannelContext, SessionId};
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
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc::Sender;

mod callback_handler;
mod config;
mod recv_agent_message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingTalkConfig {
    pub credential: dingtalk_stream::Credential,
    pub allow_session_ids: BTreeMap<String, SessionId>,
}

pub struct DingtalkChannel {
    ctx: Arc<ChannelContext>,
    dingtalk_config: DingTalkConfig,
}

impl DingtalkChannel {
    pub fn new(config: &'static Config, workspace: &'static Workspace) -> crate::Result<Self> {
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
    async fn start(
        self,
        agent: Arc<dyn Agent>,
    ) -> crate::Result<(Sender<AgentRequest>, JoinHandle<()>)> {
        let Self {
            ctx,
            dingtalk_config,
        } = self;
        let (channel_message_sender, mut channel_message_receiver) = tokio::sync::mpsc::channel(32);
        let cb_handler = Arc::new(callback_handler::DingTalkCallbackHandler {
            ctx: Arc::clone(&ctx),
            config: dingtalk_config.clone(),
            dingtalk_bot_topic: MessageTopic::Callback(dingtalk_stream::TOPIC_ROBOT.to_string()),
            agent: Arc::clone(&agent),
            channel_message_sender: channel_message_sender.clone(),
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
        let agent_request_sender = {
            let (agent_request_sender, mut agent_request_receiver) =
                tokio::sync::mpsc::channel::<AgentRequest>(1);
            tokio::spawn(async move {
                while let Some(req) = agent_request_receiver.recv().await {
                    let task_id = uuid::Uuid::new_v4().to_string();
                    match agent.run(req, channel_message_sender.clone()).await {
                        Ok(_) => {
                            info!("Agent run completed, task_id: {}", task_id);
                        }
                        Err(err) => {
                            error!("Agent run failed, task_id: {}, error: {}", task_id, err);
                        }
                    }
                }
            });
            agent_request_sender
        };

        let join_handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("unexpected err");
            let agent_handle = {
                let ctx = Arc::clone(&ctx);
                let dingtalk = Arc::clone(&dingtalk);
                rt.spawn(async move {
                    let _ = DingtalkChannel::recv_agent_message(
                        &dingtalk,
                        &ctx,
                        &mut channel_message_receiver,
                    )
                    .await;
                })
            };
            rt.block_on(async {
                let _ = dingtalk_stream_handle.await;
                let _ = agent_handle.await;
            });
        });
        Ok((agent_request_sender, join_handle))
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
