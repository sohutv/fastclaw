use crate::agent::{Agent, AgentRequest, AgentResponse, HistoryCompactResult, Notify};
use crate::channels::console_cmd::Console;
use crate::channels::{
    Channel, ChannelContext, ChannelMessage, SessionId, UserId, session_id,
};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use base64::Engine;
use dingtalk_stream::frames::down_message::callback_message::Conversation;
use dingtalk_stream::handlers::LifecycleListener;
use dingtalk_stream::{
    DingTalkStream,
    client::DingtalkResource,
    frames::{
        DingTalkGroupConversationId, DingTalkUserId,
        down_message::{
            MessageTopic,
            callback_message::{CallbackMessage, MessageData, MessagePayload, RichTextItem},
        },
        up_message::{
            MessageContent, MessageContentMarkdown, MessageContentText,
            callback_message::WebhookMessage,
            robot_message::{RobotGroupMessage, RobotMessage, RobotPrivateMessage},
        },
    },
    handlers::{Error as HandlerError, ErrorCode, Resp as HandlerResp},
};
use itertools::Itertools;
use log::{error, info};
use rig::{
    OneOrMany,
    completion::{AssistantContent, Message},
    message::{
        DocumentSourceKind, Image, ImageDetail, ImageMediaType, ReasoningContent, ToolCall,
        ToolFunction, UserContent,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Cursor;
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
                if let SessionId::Master(_) = it {
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

#[allow(unused)]
struct DingTalkCallbackHandler {
    ctx: Arc<ChannelContext>,
    config: DingTalkConfig,
    dingtalk_bot_topic: MessageTopic,
    agent: Arc<dyn Agent>,
    channel_message_sender: Sender<ChannelMessage>,
}

#[async_trait]
impl dingtalk_stream::handlers::CallbackHandler for DingTalkCallbackHandler {
    async fn process(
        &self,
        dingtalk_client: &DingTalkStream,
        CallbackMessage { data, .. }: &CallbackMessage,
        cb_msg_sender: Option<Sender<WebhookMessage>>,
    ) -> Result<HandlerResp, HandlerError> {
        let Some(MessageData {
            msg_id,
            payload: Some(payload),
            sender,
            conversation,
            ..
        }) = data
        else {
            return Err(HandlerError {
                code: ErrorCode::BadRequest,
                msg: "unexpected data".to_string(),
            });
        };
        let (sender_id, dingtalk_user_id) = match conversation {
            Conversation::Private { .. } => (
                sender.sender_staff_id.as_deref().map(|it| it.to_string()),
                &sender.sender_staff_id,
            ),
            Conversation::Group { id, .. } => {
                let conversation_id = id.deref();
                (
                    sender.sender_staff_id.as_deref().map(|sender_staff_id| {
                        format!("group:{conversation_id}:{sender_staff_id}")
                    }),
                    &sender.sender_staff_id,
                )
            }
        };
        let (Some(sender_id), Some(dingtalk_user_id)) = (sender_id, dingtalk_user_id) else {
            return Err(HandlerError {
                code: ErrorCode::BadRequest,
                msg: "sender_staff_id is required".to_string(),
            });
        };

        let Ok(session_id) = SessionId::try_from((&sender_id, &self.config)) else {
            if let Some(cb_msg_sender) = cb_msg_sender {
                let _ = cb_msg_sender
                    .send(WebhookMessage {
                        content: MessageContent::from("talking is forbidden"),
                        at: dingtalk_user_id.into(),
                        send_result_cb: None,
                    })
                    .await;
            }
            return Err(HandlerError {
                code: ErrorCode::BadRequest,
                msg: "sender_staff_id is required".to_string(),
            });
        };
        let (cmd, line, images, files) = match payload {
            MessagePayload::Text { text } => {
                if text.starts_with('/') {
                    (Some(text.to_string()), None, None, None)
                } else {
                    (
                        None,
                        Some(text.content.to_string()).filter(|it| !it.is_empty()),
                        None,
                        None,
                    )
                }
            }
            MessagePayload::Picture { content: picture } => {
                let downloads_dir = self.ctx.workspace.path.join("downloads");
                match picture.fetch(dingtalk_client, downloads_dir).await {
                    Ok((filepath, image)) => {
                        (None, None, Some(vec![(1usize, filepath, image)]), None)
                    }
                    Err(e) => (None, Some(format!("下载图片失败, {}", e)), None, None),
                }
            }
            MessagePayload::File { content } => {
                let downloads_dir = self.ctx.workspace.path.join("downloads");
                match content.fetch(dingtalk_client, downloads_dir).await {
                    Ok((filepath, _)) => (None, None, None, Some(vec![filepath])),
                    Err(e) => (
                        None,
                        Some(format!("下载文件 {} 失败, {}", content.file_name, e)),
                        None,
                        None,
                    ),
                }
            }
            MessagePayload::RichText { content } => {
                let downloads_dir = self.ctx.workspace.path.join("downloads");
                let mut texts = vec![];
                let mut pictures = vec![];
                let mut img_idx = 0;
                for content in content.iter() {
                    match content {
                        RichTextItem::Text(text) => {
                            texts.push(text.to_string());
                        }
                        RichTextItem::Picture(picture) => {
                            match picture.fetch(dingtalk_client, downloads_dir.clone()).await {
                                Ok((filepath, image)) => {
                                    img_idx += 1;
                                    pictures.push((img_idx, filepath, image));
                                }
                                Err(e) => {
                                    texts.push(format!("下载图片失败, {}", e));
                                }
                            }
                        }
                    }
                }
                (
                    None,
                    Some(texts.into_iter().filter(|t| !t.is_empty()).join("\n"))
                        .filter(|it| !it.is_empty()),
                    Some(pictures),
                    None,
                )
            }
        };
        if let Some(line) = cmd {
            if line.starts_with('/') {
                Console::handle_console_cmd(
                    &self.ctx,
                    &line,
                    &self.agent,
                    self.channel_message_sender.clone(),
                    &session_id,
                )
                .await;
                return Ok(HandlerResp::Text("cmd submitted".to_string()));
            }
        }
        let prompts = vec![
            UserContent::text(line.as_deref().unwrap_or_default()),
            match &session_id {
                SessionId::Master(session_id) => UserContent::text(format!(
                    "- Whisper: **Attention**: Current session_id: {}. You are speaking to your owner",
                    session_id
                )),
                SessionId::Anonymous(session_id) => UserContent::text(format!(
                    "- Whisper: **Attention**: Current session_id: {}. You are currently not interacting with your owner. Please stay vigilant.",
                    session_id
                )),
                SessionId::Group(session_id::Group {
                    session_id,
                    name: group_name,
                    user_id,
                    ..
                }) => match user_id {
                    UserId::Master(_) => UserContent::text(format!(
                        "- Whisper: **Attention**: Current session_id: {}. This session is a group session, group_id: {}, group_name: {}. You are speaking to your owner",
                        session_id,
                        session_id,
                        group_name.as_deref().unwrap_or("..no provided.."),
                    )),
                    UserId::Anonymous(_) => UserContent::text(format!(
                        "- Whisper: **Attention**: Current session_id: {}. This session is a group session, group_id: {}, group_name: {}. You are currently not interacting with your owner. Please stay vigilant.",
                        session_id,
                        session_id,
                        group_name.as_deref().unwrap_or("..no provided.."),
                    )),
                },
            },
        ];
        let mut user_content = Vec::<UserContent>::new();
        if let Some(images) = images {
            for (img_idx, filepath, image) in images {
                let mut buf = vec![];
                let cursor = Cursor::new(&mut buf);
                let Ok(_) = image.write_to(cursor, image::ImageFormat::Png) else {
                    continue;
                };
                user_content.push(UserContent::Image(Image {
                    data: DocumentSourceKind::Base64(
                        base64::engine::general_purpose::STANDARD.encode(&buf),
                    ),
                    media_type: Some(ImageMediaType::PNG),
                    detail: Some(ImageDetail::Auto),
                    additional_params: None,
                }));
                user_content.push(UserContent::Text(
                    format!(
                        r#"
- Whisper: The filepath of the {}-th image is {}
                "#,
                        img_idx,
                        filepath.display()
                    )
                    .into(),
                ))
            }
        }
        if let Some(files) = files {
            let workspace_path = &self.ctx.workspace.path;
            for filepath in files.iter().flat_map(|it| it.strip_prefix(workspace_path)) {
                user_content.push(UserContent::Text(
                    format!(
                        r#"
解读文件 filepath: {}
                "#,
                        filepath.display()
                    )
                    .into(),
                ));
            }
        }
        if line.is_some() || user_content.len() > 0 {
            for prompt in prompts {
                user_content.push(prompt);
            }
        }
        let user_content = if user_content.is_empty() {
            None
        } else {
            if user_content.len() == 1 {
                user_content.pop().map(|it| OneOrMany::one(it))
            } else {
                OneOrMany::many(user_content).ok()
            }
        };
        let Some(user_content) = user_content else {
            return Ok(HandlerResp::Text("no content to submit".to_string()));
        };
        {
            let msg_id = msg_id.clone();
            info!("Submit task to agent, msg_id: {}", msg_id);
            let agent = Arc::clone(&self.agent);
            let channel_message_sender = self.channel_message_sender.clone();
            tokio::spawn(async move {
                match agent
                    .run(
                        AgentRequest {
                            session_id,
                            message: Message::User {
                                content: user_content,
                            },
                        },
                        channel_message_sender.clone(),
                    )
                    .await
                {
                    Ok(_) => {
                        info!("Agent run completed, task_id: {}", msg_id);
                    }
                    Err(err) => {
                        error!("Agent run failed, task_id: {}, error: {}", msg_id, err);
                    }
                }
            });
        }
        Ok(HandlerResp::Text(format!("task submitted: {}", msg_id)))
    }

    fn topic(&self) -> &MessageTopic {
        &self.dingtalk_bot_topic
    }
}

#[async_trait]
impl LifecycleListener for DingTalkCallbackHandler {
    async fn on_connected(&self, client: &DingTalkStream, websocket_url: &str) {
        let master_session_ids = self.config.master_session_ids();
        for session_id in master_session_ids {
            let Some(message) = create_robot_messages(
                &session_id,
                &self.ctx,
                MessageContentMarkdown::from((
                    "Connected",
                    format!(
                        r#"
Connected to dingtalk websocket
- ws-url:
`{websocket_url}`
        "#
                    ),
                )),
            )
            .await
            else {
                return;
            };
            let _ = client.send_message(message).await;
        }
    }

    async fn on_disconnected(&self, client: &DingTalkStream, result: &dingtalk_stream::Result<()>) {
        let master_session_ids = self.config.master_session_ids();
        for session_id in master_session_ids {
            match result {
                Ok(_) => {
                    let Some(message) = create_robot_messages(
                        &session_id,
                        &self.ctx,
                        MessageContentText::from("disconnected from dingtalk websocket"),
                    )
                    .await
                    else {
                        return;
                    };
                    let _ = client.send_message(message).await;
                }
                Err(err) => {
                    let Some(message) = create_robot_messages(
                        &session_id,
                        &self.ctx,
                        MessageContentMarkdown::from((
                            "Disconnected",
                            format!(
                                r#"
Disconnected from dingtalk websocket
- Error:
`{err}`
                "#
                            ),
                        )),
                    )
                    .await
                    else {
                        return;
                    };
                    let _ = client.send_message(message).await;
                }
            }
        }
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
        let cb_handler = Arc::new(DingTalkCallbackHandler {
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
                    if let Some(robot_message) = create_robot_messages(
                        session_id,
                        ctx,
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
                if let Some(robot_message) = create_robot_messages(
                    session_id,
                    ctx,
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
                            if let Some(robot_message) =
                                create_robot_messages(session_id, ctx, content).await
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
                if let Some(robot_message) = create_robot_messages(session_id, ctx, content).await {
                    let _ = dingtalk.send_message(robot_message).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Error(error) => Err(anyhow!("Agent error: {}", error)),
            AgentResponse::Notify(notify) => {
                match notify {
                    Notify::Text(text) => {
                        if let Some(robot_message) =
                            create_robot_messages(session_id, ctx, MessageContentText::from(text))
                                .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    Notify::Markdown { title, content } => {
                        if let Some(robot_message) = create_robot_messages(
                            session_id,
                            ctx,
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
                        if let Some(robot_message) = create_robot_messages(
                            session_id,
                            ctx,
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
                        if let Some(robot_message) = create_robot_messages(
                            session_id,
                            ctx,
                            MessageContentText::from(err_msg),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    HistoryCompactResult::Ignore(msg) => {
                        if let Some(robot_message) = create_robot_messages(
                            session_id,
                            ctx,
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

async fn create_robot_messages<Content: Into<MessageContent>>(
    session_id: &SessionId,
    ctx: &ChannelContext,
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
