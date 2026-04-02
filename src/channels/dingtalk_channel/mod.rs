use crate::agent::{Agent, AgentRequest, AgentResponse, HistoryCompactResult, Notify};
use crate::channels::{Channel, ChannelContext, ChannelMessage, SessionId, SessionSettings};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use dingtalk_stream::{
    DingTalkStream,
    frames::{
        DingTalkGroupConversationId, DingTalkUserId,
        down_message::MessageTopic,
        up_message::{
            MessageContent, MessageContentMarkdown, MessageContentText,
            robot_message::{RobotGroupMessage, RobotMessage, RobotPrivateMessage},
        },
    },
};
use itertools::Itertools;
use log::{error, info};
use rig::{
    completion::{AssistantContent, Message},
    message::{ReasoningContent, ToolCall, ToolFunction},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingTalkConfig {
    pub credential: dingtalk_stream::Credential,
    pub allow_session_ids: BTreeMap<String, SessionId>,
}

impl DingTalkConfig {
    fn allow_session_id<UserId: AsRef<str>>(&self, user_id: UserId) -> Option<&SessionId> {
        self.allow_session_ids.get(user_id.as_ref())
    }

    fn master_session_ids(&self) -> Vec<&SessionId> {
        self.allow_session_ids
            .values()
            .flat_map(|it| {
                if let SessionId::Master { .. } = it {
                    Some(it)
                } else {
                    None
                }
            })
            .collect_vec()
    }
}

impl<S: AsRef<str>> TryFrom<(S, &DingTalkConfig)> for SessionId {
    type Error = anyhow::Error;

    fn try_from((session_id_key, config): (S, &DingTalkConfig)) -> Result<Self, Self::Error> {
        match config.allow_session_id(session_id_key.as_ref()) {
            Some(dst) => Ok(dst.clone()),
            None => Err(anyhow!(
                "session_id {} not allowed",
                session_id_key.as_ref()
            )),
        }
    }
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

mod callback_handler;

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
                    let _ = DingtalkChannel::poll_agent_message(
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
impl DingtalkChannel {}
impl DingtalkChannel {
    async fn poll_agent_message(
        dingtalk: &DingTalkStream,
        ctx: &ChannelContext,
        receiver: &mut Receiver<ChannelMessage>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message) = receiver.recv().await {
            match Self::handle_agent_message(dingtalk, &*ctx, &message, state, &mut buff).await {
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
        dingtalk: &DingTalkStream,
        ctx: &ChannelContext,
        ChannelMessage {
            session_id,
            message,
        }: &ChannelMessage,
        curr_state: AgentRespState,
        buff: &mut Vec<String>,
    ) -> crate::Result<AgentRespState> {
        match message {
            AgentResponse::Start => {
                if let AgentRespState::Wait = curr_state {
                    buff.clear();
                    if let Some(robot_message) = create_robot_messages_for_agent(
                        session_id,
                        ctx,
                        AgentRespType::Start,
                        MessageContentText::from("正在思考..."),
                    )
                    .await
                    {
                        let _ = dingtalk.send_message(robot_message).await;
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
                if let Some(robot_message) = create_robot_messages_for_agent(
                    session_id,
                    ctx,
                    AgentRespType::ToolCall,
                    MessageContentMarkdown::from((
                        format!("工具调用: {name}..."),
                        format!(
                            r#"
### 工具调用: {name}...
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
                )
                .await
                {
                    let _ = dingtalk.send_message(robot_message).await;
                }
                Ok(curr_state)
            }
            AgentResponse::ReasoningStream(reasoning) => {
                match curr_state {
                    AgentRespState::Start => if ctx.config.default_show_reasoning {},
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
                        if ctx.config.default_show_reasoning {
                            let content = {
                                let content = buff.join("");
                                buff.clear();
                                MessageContentMarkdown::from((
                                    "正在思考...",
                                    format!(
                                        r#"
### 我的想法..
{content}
                                    "#
                                    ),
                                ))
                            };
                            if let Some(robot_message) = create_robot_messages_for_agent(
                                session_id,
                                ctx,
                                AgentRespType::Reasoning,
                                content,
                            )
                            .await
                            {
                                let _ = dingtalk.send_message(robot_message).await;
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
                    let content = MessageContentMarkdown::from((
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
                if let Some(robot_message) = create_robot_messages_for_agent(
                    session_id,
                    ctx,
                    AgentRespType::Content,
                    content,
                )
                .await
                {
                    let _ = dingtalk.send_message(robot_message).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Error(error) => {
                if let Some(robot_message) = create_robot_messages_for_agent(
                    session_id,
                    ctx,
                    AgentRespType::Error,
                    MessageContentText::from(format!("Agent error: {}", error)),
                )
                .await
                {
                    let _ = dingtalk.send_message(robot_message).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Notify(notify) => {
                match notify {
                    Notify::Text(text) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::Notify,
                            MessageContentText::from(text),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    Notify::Markdown { title, content } => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::Notify,
                            MessageContentMarkdown::from((title, &format!("{content}",))),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                }
                Ok(curr_state)
            }
            AgentResponse::HistoryCompact(result) => {
                match result {
                    HistoryCompactResult::Ok(val) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::HistoryCompactOk,
                            MessageContentMarkdown::from((
                                "压缩上下文完成",
                                &format!(
                                    r#"
### 压缩上下文完成
- 压缩前 **{}** Tokens
- 压缩后 **{}** Tokens
- 压缩率 **{:.2}%**
                    "#,
                                    val.before().total_tokens,
                                    val.current().total_tokens,
                                    val.compact_ratio(),
                                ),
                            )),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    HistoryCompactResult::Err(err_msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::HistoryCompactErr,
                            MessageContentText::from(err_msg),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    HistoryCompactResult::Ignore(msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::HistoryCompactIgnore,
                            MessageContentMarkdown::from((
                                "压缩请求被忽略",
                                format!(
                                    r#"
### 压缩请求被忽略
{msg}
                            "#
                                ),
                            )),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                }

                Ok(curr_state)
            }
        }
    }
}

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
async fn create_robot_messages_for_agent<Content: Into<MessageContent>>(
    session_id: &SessionId,
    ctx: &ChannelContext,
    resp_type: AgentRespType,
    content: Content,
) -> Option<RobotMessage> {
    let Some(session_id) = ctx
        .config
        .dingtalk_config
        .as_ref()
        .and_then(|cfg| SessionId::try_from((session_id.deref(), cfg)).ok())
    else {
        return None;
    };

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
    } = session_id.settings();
    match resp_type {
        AgentRespType::Start => {
            let true = show_start else {
                return None;
            };
        }
        AgentRespType::ToolCall => {
            let true = show_toolcall else {
                return None;
            };
        }
        AgentRespType::Reasoning => {
            let true = show_reasoning else {
                return None;
            };
        }
        AgentRespType::Content => {}
        AgentRespType::Notify => {
            let true = show_notify else {
                return None;
            };
        }
        AgentRespType::HistoryCompactOk => {
            let true = (*show_compacting && *show_compacting_ok) else {
                return None;
            };
        }
        AgentRespType::HistoryCompactErr => {
            let true = (*show_compacting && *show_compacting_err) else {
                return None;
            };
        }
        AgentRespType::HistoryCompactIgnore => {
            let true = (*show_compacting && *show_compacting_ignore) else {
                return None;
            };
        }
        AgentRespType::Error => {
            let true = show_error else {
                return None;
            };
        }
    }
    create_robot_messages(&session_id, ctx, content).await
}
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

#[derive(Debug, Clone, Copy)]
enum AgentRespState {
    Wait,
    Start,
    Reasoning,
    Messaging,
    Final,
}
