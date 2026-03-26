use crate::agent::{Agent, AgentRequest, AgentResponse};
use crate::channels::console_cmd::Console;
use crate::channels::{Channel, ChannelContext, ChannelMessage, Session, SessionId};
use crate::config::{Config, DingTalkConfig};
use anyhow::anyhow;
use async_trait::async_trait;
use dingtalk_stream::client::DingtalkMessageSender;
use dingtalk_stream::frames::{
    CallbackMessageConversation, CallbackMessageData, CallbackMessagePayload,
    CallbackWebhookMessage, DingTalkGroupConversationId, DingTalkUserId, RichTextItem,
    RobotGroupMessage, RobotMessage, RobotPrivateMessage, UpMessageContent,
    UpMessageContentMarkdown,
};
use dingtalk_stream::{CallbackMessage, DingTalkStream, Error, ErrorCode, MessageTopic, Resp};
use itertools::Itertools;
use log::warn;
use rig::completion::{AssistantContent, Message};
use rig::message::{ReasoningContent, ToolCall, ToolFunction};
use std::ops::Deref;
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::RwLock;
use tokio::sync::mpsc::{Receiver, Sender};

pub struct DingtalkChannel {
    ctx: Arc<RwLock<ChannelContext>>,
    dingtalk_config: DingTalkConfig,
}

impl DingtalkChannel {
    pub fn new(config: &'static Config) -> crate::Result<Self> {
        Ok(Self {
            ctx: Arc::new(RwLock::new(ChannelContext {
                config: config.clone(),
                sessions: Default::default(),
            })),
            dingtalk_config: config
                .dingtalk_config
                .clone()
                .ok_or(anyhow!("dingtalk config not found"))?,
        })
    }
}

#[allow(unused)]
struct DingTalkCallbackHandler {
    ctx: Arc<RwLock<ChannelContext>>,
    dingtalk_config: DingTalkConfig,
    dingtalk_bot_topic: MessageTopic,
    agent: Box<dyn Agent>,
    channel_message_sender: Sender<ChannelMessage>,
}

#[async_trait]
impl dingtalk_stream::CallbackHandler for DingTalkCallbackHandler {
    async fn process(
        &self,
        CallbackMessage { data, .. }: &CallbackMessage,
        cb_msg_sender: Option<Sender<CallbackWebhookMessage>>,
    ) -> Result<Resp, Error> {
        let Some(CallbackMessageData {
            msg_id: _,
            payload: Some(payload),
            sender,
            conversation,
            ..
        }) = data
        else {
            return Err(Error {
                code: ErrorCode::BadRequest,
                msg: "unexpected data".to_string(),
            });
        };
        let Some(dingtalk_user_id) = &sender.sender_staff_id else {
            return Err(Error {
                code: ErrorCode::BadRequest,
                msg: "sender_staff_id is required".to_string(),
            });
        };
        let DingTalkConfig {
            master_user_id,
            allow_user_ids,
            ..
        } = &self.dingtalk_config;
        let is_master = master_user_id.eq(dingtalk_user_id.deref());
        if let (0, false, Some(cb_msg_sender)) = (
            allow_user_ids
                .iter()
                .filter(|&it| it.eq(dingtalk_user_id.deref()))
                .count(),
            is_master,
            cb_msg_sender,
        ) {
            let _ = cb_msg_sender
                .send(CallbackWebhookMessage {
                    content: UpMessageContent::from("forbidden, not allowed"),
                    at: dingtalk_user_id.into(),
                    send_result_cb: None,
                })
                .await;
        }
        let session_id = {
            let mut ctx = self.ctx.write().await;
            let session = match conversation {
                CallbackMessageConversation::Private { .. } => Session::Private {
                    session_id: SessionId::from(dingtalk_user_id.deref().to_string()),
                },
                CallbackMessageConversation::Group { id, .. } => Session::Group {
                    session_id: SessionId::from(id.deref().to_string()),
                },
            };
            let session_id = session.deref().clone();
            ctx.sessions.entry(session_id.clone()).or_insert(session);
            session_id
        };
        let line = match payload {
            CallbackMessagePayload::Text { text } => text.content.to_string(),
            CallbackMessagePayload::Picture { .. } => "".to_string(),
            CallbackMessagePayload::File { .. } => "".to_string(),
            CallbackMessagePayload::RichText { content } => content
                .content
                .iter()
                .map(|it| match it {
                    RichTextItem::Picture { .. } => "".to_string(),
                    RichTextItem::Text { text } => text.to_string(),
                })
                .join(""),
        };
        let line = line.trim();
        if line.is_empty() {
            return Ok(Resp::Text("ignore empty".to_string()));
        }
        if line.starts_with('/') {
            Console::handle_console_cmd(
                Arc::clone(&self.ctx),
                &line,
                &self.agent,
                self.channel_message_sender.clone(),
                &session_id,
            )
            .await;
            return Ok(Resp::Text("cmd submitted".to_string()));
        }
        let line = if is_master {
            format!(
                r#"
{line}
- Whisper: **Attention**: Current session_id: {session_id}. You are speaking to your owner
"#
            )
        } else {
            format!(
                r#"
{line}
- Whisper: **Attention**: You are currently not interacting with your owner. Please stay vigilant.
"#
            )
        };
        match self
            .agent
            .run(
                AgentRequest {
                    session_id,
                    message: Message::user(line),
                },
                self.channel_message_sender.clone(),
            )
            .await
        {
            Ok(()) => Ok(Resp::Text("task submitted".to_string())),
            Err(err) => Ok(Resp::Text(format!("submit task failed: {err}"))),
        }
    }

    fn topic(&self) -> &MessageTopic {
        &self.dingtalk_bot_topic
    }
}

#[async_trait]
impl Channel for DingtalkChannel {
    async fn start(self, agent: Box<dyn Agent>) -> crate::Result<JoinHandle<()>> {
        let Self {
            ctx,
            dingtalk_config,
        } = self;
        let (channel_message_sender, mut channel_message_receiver) = tokio::sync::mpsc::channel(32);
        let cb_handler = {
            let ctx = Arc::clone(&ctx);
            DingTalkCallbackHandler {
                ctx,
                dingtalk_config: dingtalk_config.clone(),
                dingtalk_bot_topic: MessageTopic::Callback(
                    dingtalk_stream::TOPIC_ROBOT.to_string(),
                ),
                agent,
                channel_message_sender,
            }
        };
        let (mut dingtalk_stream, dingtalk_msg_sender) =
            DingTalkStream::new(dingtalk_config.credential)
                .register_callback_handler(cb_handler)
                .create_message_sender()
                .await;

        let join_handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("unexpected err");
            let dingtalk_stream_handle = {
                rt.spawn(async move {
                    let stop_tx = Arc::clone(&dingtalk_stream.stop_tx);
                    tokio::spawn(async move {
                        dingtalk_stream.start_forever().await;
                    });
                    let _ = tokio::signal::ctrl_c().await;
                    let stop_tx = stop_tx.lock().await;
                    if let Some(stop_tx) = stop_tx.as_ref() {
                        let _ = stop_tx.send(()).await;
                    }
                })
            };
            let agent_handle = {
                let ctx = Arc::clone(&ctx);
                rt.spawn(async move {
                    let _ = DingtalkChannel::poll_agent_message(
                        &ctx,
                        &mut channel_message_receiver,
                        &dingtalk_msg_sender,
                    )
                    .await;
                })
            };
            rt.block_on(async {
                let _ = dingtalk_stream_handle.await;
                let _ = agent_handle.await;
            });
        });
        Ok(join_handle)
    }
}

impl DingtalkChannel {
    async fn poll_agent_message(
        ctx: &Arc<RwLock<ChannelContext>>,
        receiver: &mut Receiver<ChannelMessage>,
        dingtalk_msg_sender: &DingtalkMessageSender,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message) = receiver.recv().await {
            let ctx = ctx.read().await;
            match Self::handle_agent_message(
                &*ctx,
                &message,
                state,
                &mut buff,
                &dingtalk_msg_sender,
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
        Ok(())
    }

    async fn handle_agent_message(
        ctx: &ChannelContext,
        ChannelMessage {
            session_id,
            message,
        }: &ChannelMessage,
        curr_state: AgentRespState,
        buff: &mut Vec<String>,
        dingtalk_msg_sender: &DingtalkMessageSender,
    ) -> crate::Result<AgentRespState> {
        match message {
            AgentResponse::Start => {
                if let AgentRespState::Wait = curr_state {
                    buff.clear();
                    if let Some(robot_message) = Self::create_robot_messages(
                        session_id,
                        ctx,
                        UpMessageContentMarkdown::from(("思考中...", "正在思考...")),
                    ) {
                        let _ = dingtalk_msg_sender.send(robot_message).await;
                    }
                    Ok(AgentRespState::Start)
                } else {
                    Err(anyhow!("AgentRespState must be Init when starting"))
                }
            }
            AgentResponse::ToolCall(ToolCall {
                function: ToolFunction { name, arguments },
                ..
            }) => {
                if let Some(robot_message) = Self::create_robot_messages(
                    session_id,
                    ctx,
                    UpMessageContentMarkdown::from((
                        format!("思考中...(工具调用: {name})"),
                        format!(
                            r#"
### 工具调用: {name}
```
{}
```json
                                            "#,
                            serde_json::to_string_pretty(arguments).unwrap_or_else(|err| format!(
                                "Error serializing arguments: {}",
                                err
                            ))
                        ),
                    )),
                ) {
                    let _ = dingtalk_msg_sender.send(robot_message).await;
                }
                Ok(curr_state)
            }
            AgentResponse::ReasoningStream(reasoning) => {
                match curr_state {
                    AgentRespState::Start => if ctx.config.show_reasoning {},
                    _ => {}
                }
                for content in reasoning.content.iter() {
                    if let ReasoningContent::Text { text, .. } = content {
                        if !text.is_empty() {
                            buff.push(text.clone());
                        }
                    }
                }
                Ok(AgentRespState::Reasoning)
            }
            AgentResponse::MessageStream(message) => {
                match curr_state {
                    AgentRespState::Start => {}
                    AgentRespState::Reasoning => {
                        if ctx.config.show_reasoning {
                            let content = {
                                let content = buff.join("");
                                buff.clear();
                                UpMessageContentMarkdown::from((
                                    "思考完成...",
                                    format!(
                                        r#"
### 正在思考...
{content}
                                    "#
                                    ),
                                ))
                            };
                            if let Some(robot_message) =
                                Self::create_robot_messages(session_id, ctx, content)
                            {
                                let _ = dingtalk_msg_sender.send(robot_message).await;
                            }
                        }
                    }
                    _ => {}
                }
                match message {
                    Message::Assistant { content, .. } => {
                        for content in content.iter() {
                            match content {
                                AssistantContent::Text(text) => {
                                    let text_str = text.to_string();
                                    if !text_str.is_empty() {
                                        buff.push(text_str);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                Ok(AgentRespState::Messaging)
            }
            AgentResponse::Final(usage) => {
                let content = {
                    let content = UpMessageContentMarkdown::from((
                        "回复中...",
                        format!(
                            r#"
{}

*<<Tokens:{}↑{}↓{}>>*
                    "#,
                            buff.join(""),
                            usage.total_tokens,
                            usage.input_tokens,
                            usage.output_tokens
                        ),
                    ));
                    buff.clear();
                    content
                };
                if let Some(robot_message) = Self::create_robot_messages(session_id, ctx, content) {
                    let _ = dingtalk_msg_sender.send(robot_message).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Error(error) => {
                eprintln!("{}", error);
                Err(anyhow!("Agent error: {}", error))
            }
            AgentResponse::HistoryCompact { before, after } => {
                if let Some(robot_message) = Self::create_robot_messages(
                    session_id,
                    ctx,
                    UpMessageContentMarkdown::from((
                        "压缩上下文",
                        &format!(
                            r#"
### 压缩上下文完成
- 压缩前 **{}** Tokens
- 压缩后 **{}** Tokens
- 压缩率 **{:.2}%**
                    "#,
                            before.total_tokens,
                            after.total_tokens,
                            (before.total_tokens as f32 / after.total_tokens as f32) * 100.
                        ),
                    )),
                ) {
                    let _ = dingtalk_msg_sender.send(robot_message).await;
                }
                Ok(curr_state)
            }
        }
    }

    fn create_robot_messages<Content: Into<UpMessageContent>>(
        session_id: &SessionId,
        ctx: &ChannelContext,
        content: Content,
    ) -> Option<RobotMessage> {
        let Some(session) = ctx.sessions.get(session_id) else {
            warn!("Session not found for ID: {}", session_id);
            return None;
        };
        let content = content.into();
        match session {
            Session::Private { session_id } => Some(
                RobotPrivateMessage {
                    user_ids: vec![DingTalkUserId::from(session_id.deref())],
                    content: content.clone(),
                    send_result_cb: None,
                }
                .into(),
            ),
            Session::Group { session_id } => Some(
                RobotGroupMessage {
                    group_id: DingTalkGroupConversationId::from(session_id.deref()),
                    content: content.clone(),
                    send_result_cb: None,
                }
                .into(),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AgentRespState {
    Wait,
    Start,
    Reasoning,
    Messaging,
    Final,
}
