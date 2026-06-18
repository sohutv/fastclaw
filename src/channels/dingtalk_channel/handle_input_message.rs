use crate::agent::{AgentRequest, AgentVisitor, DelegatedAgent};
use crate::channels::console_cmd::Console;
use crate::channels::dingtalk_channel::DingtalkChannel;
use crate::channels::{Channel, GroupUserId, SessionId, session_id};
use async_trait::async_trait;
use base64::Engine;
use dingtalk_stream::frames::DingTalkUserId;
use dingtalk_stream::frames::down_message::callback_message::MessageSender;
use dingtalk_stream::{
    DingTalkStream,
    client::DingtalkResource,
    frames::{
        down_message::{
            MessageTopic,
            callback_message::{
                CallbackMessage, Conversation, MessageData, MessagePayload, RichTextItem,
            },
        },
        up_message::{
            MessageContent, MessageContentMarkdown, MessageContentText,
            callback_message::WebhookMessage,
        },
    },
    handlers::{Error as HandlerError, ErrorCode, LifecycleListener, Resp as HandlerResp},
};
use log::{info, warn};
use rig::OneOrMany;
use rig::message::{DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent};
use std::io::Cursor;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

#[allow(unused)]
pub(super) struct DingTalkCallbackHandler {
    pub(super) channel: &'static DingtalkChannel,
    pub(super) dingtalk_bot_topic: MessageTopic,
}

#[async_trait]
impl dingtalk_stream::handlers::CallbackHandler for DingTalkCallbackHandler {
    async fn process(
        &self,
        dingtalk_client: Arc<DingTalkStream>,
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
        let session_id = {
            let (sender_id, dingtalk_user_id) = parse_sender_id(sender, conversation)?;
            let Ok(session_id) = SessionId::try_from((&sender_id, &self.channel.dingtalk_config))
            else {
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
            session_id
        };
        let (cmd, user_contents) = {
            let mut cmd: Option<String> = None;
            let mut user_contents = vec![];
            match payload {
                MessagePayload::Text { text } => {
                    if !text.is_empty() {
                        if text.starts_with('/') {
                            cmd = Some(text.to_string());
                        }
                        user_contents.push(UserContent::text(text.to_string()));
                    }
                }
                MessagePayload::Picture { content: picture } => {
                    let downloads_dir = self
                        .channel
                        .context
                        .workspace
                        .downloads_path()
                        .to_path_buf();
                    match picture.fetch(&dingtalk_client, downloads_dir).await {
                        Ok((filepath, image)) => {
                            let mut buf = vec![];
                            let cursor = Cursor::new(&mut buf);
                            if let Ok(_) = image.write_to(cursor, image::ImageFormat::Png) {
                                user_contents.push(UserContent::Image(Image {
                                    data: DocumentSourceKind::Base64(
                                        base64::engine::general_purpose::STANDARD.encode(&buf),
                                    ),
                                    media_type: Some(ImageMediaType::PNG),
                                    detail: Some(ImageDetail::Auto),
                                    additional_params: None,
                                }));
                                user_contents.push(UserContent::Text(
                                    format!(
                                        "- **filepath of the input image**: {}",
                                        filepath.display()
                                    )
                                    .into(),
                                ));
                            }
                        }
                        Err(e) => {
                            UserContent::text(format!("download picture failed, {}", e));
                        }
                    }
                }
                MessagePayload::Video { content } => {
                    let downloads_dir = self
                        .channel
                        .context
                        .workspace
                        .downloads_path()
                        .to_path_buf();
                    match content.fetch(&dingtalk_client, downloads_dir).await {
                        Ok((filepath, _)) => {
                            user_contents.push(UserContent::Text(
                                format!(
                                    "- **filepath of the input video**: {}",
                                    filepath.display()
                                )
                                .into(),
                            ));
                        }
                        Err(e) => {
                            UserContent::text(format!("download video failed, {}", e));
                        }
                    }
                }
                MessagePayload::Audio { content } => {
                    let text = &content.recognition;
                    if !text.is_empty() {
                        user_contents.push(UserContent::text(text));
                    }
                    let downloads_dir = self
                        .channel
                        .context
                        .workspace
                        .downloads_path()
                        .to_path_buf();
                    if let Err(err) = content.fetch(&dingtalk_client, downloads_dir).await {
                        warn!("download audio failed, {err}");
                    }
                }
                MessagePayload::File { content } => {
                    let downloads_dir = self
                        .channel
                        .context
                        .workspace
                        .downloads_path()
                        .to_path_buf();
                    match content.fetch(&dingtalk_client, downloads_dir).await {
                        Ok((filepath, _)) => {
                            user_contents.push(UserContent::Text(
                                format!("- **filepath of input file**: {}", filepath.display())
                                    .into(),
                            ));
                        }
                        Err(e) => {
                            UserContent::text(format!(
                                "download file {} failed, {}",
                                content.file_name, e
                            ));
                        }
                    }
                }
                MessagePayload::RichText { content } => {
                    let downloads_dir = self
                        .channel
                        .context
                        .workspace
                        .downloads_path()
                        .to_path_buf();
                    let mut texts = vec![];
                    for content in content.iter() {
                        match content {
                            RichTextItem::Text(text) => {
                                if !text.is_empty() {
                                    user_contents.push(UserContent::text(text.to_string()));
                                }
                            }
                            RichTextItem::Picture(picture) => {
                                match picture.fetch(&dingtalk_client, downloads_dir.clone()).await {
                                    Ok((filepath, image)) => {
                                        let mut buf = vec![];
                                        let cursor = Cursor::new(&mut buf);
                                        if let Ok(_) =
                                            image.write_to(cursor, image::ImageFormat::Png)
                                        {
                                            user_contents.push(UserContent::Image(Image {
                                                data: DocumentSourceKind::Base64(
                                                    base64::engine::general_purpose::STANDARD
                                                        .encode(&buf),
                                                ),
                                                media_type: Some(ImageMediaType::PNG),
                                                detail: Some(ImageDetail::Auto),
                                                additional_params: None,
                                            }));
                                            user_contents.push(UserContent::Text(
                                                format!(
                                                    "- **filepath of the input image**: {}",
                                                    filepath.display()
                                                )
                                                .into(),
                                            ));
                                        };
                                    }
                                    Err(e) => {
                                        texts.push(format!("download picture failed, {}", e));
                                    }
                                }
                            }
                        }
                    }
                }
            };
            (cmd, user_contents)
        };
        if let Some(cmd_val) = &cmd {
            match Console::handle_console_cmd(
                &self.channel.context,
                &cmd_val,
                self.channel.agent.delegated(),
                &session_id,
            )
            .await
            {
                Ok(mut receiver) => {
                    let client = Arc::clone(&dingtalk_client);
                    let channel = self.channel;
                    let _ = tokio::spawn(async move {
                        let _ = channel.handle_agent_message(client, &mut receiver).await;
                    });
                    return Ok(HandlerResp::Text("cmd submitted".to_string()));
                }
                Err(_) => {}
            }
        }

        let user_contents = match OneOrMany::many(user_contents) {
            Ok(val) => val,
            Err(_) => {
                return Ok(HandlerResp::Text("no content to submit".to_string()));
            }
        };

        let addi_preamble = match &session_id {
            SessionId::Master(session_id) => format!(
                "- **Attention current session_id**: {session_id}. You are speaking to your owner",
            ),
            SessionId::Anonymous(session_id) => format!(
                "- **Attention current session_id**: {session_id}. You are currently not interacting with your owner. Please stay vigilant.",
            ),
            SessionId::Group(session_id::Group {
                id: group_id,
                name: group_name,
                user_id,
                ..
            }) => match user_id {
                GroupUserId::Master(_) => format!(
                    "- **Attention current session_id**: {}. This session is a group session, group_id: {}, group_name: {}. You are speaking to your owner",
                    session_id,
                    group_id,
                    group_name.as_deref().unwrap_or("<unknown>"),
                ),
                GroupUserId::Anonymous(_) => format!(
                    "- **Attention current session_id**: {}. This session is a group session, group_id: {}, group_name: {}. You are currently not interacting with your owner. Please stay vigilant.",
                    session_id,
                    group_id,
                    group_name.as_deref().unwrap_or("<unknown>"),
                ),
            },
        }.into();

        let msg_id = msg_id.clone();
        info!("Submit task to agent, msg_id: {}", msg_id);
        match self
            .channel
            .spawn_agent_request(
                &dingtalk_client,
                self.channel.agent.id(),
                AgentRequest {
                    id: msg_id.to_string().into(),
                    session_id,
                    message: vec![user_contents],
                    addi_preamble: Some(addi_preamble),
                },
            )
            .await
        {
            Ok(_) => {
                let msg = format!("Submit agent task ok, msg_id: {}", msg_id);
                info!("{msg}");
                Ok(HandlerResp::Text(msg))
            }
            Err(err) => {
                warn!("Agent run failed, msg_id: {}, error: {}", msg_id, err);
                Ok(HandlerResp::Text(format!(
                    "Submit agent task failed, msg_id: {}",
                    msg_id
                )))
            }
        }
    }

    fn topic(&self) -> &MessageTopic {
        &self.dingtalk_bot_topic
    }
}

fn parse_sender_id<'a>(
    sender: &'a MessageSender,
    conversation: &Conversation,
) -> Result<(String, &'a DingTalkUserId), HandlerError> {
    let (sender_id, dingtalk_user_id) = match conversation {
        Conversation::Private { .. } => (
            sender.sender_staff_id.as_deref().map(|it| it.to_string()),
            &sender.sender_staff_id,
        ),
        Conversation::Group { id, .. } => {
            let conversation_id = id.deref();
            (
                sender
                    .sender_staff_id
                    .as_deref()
                    .map(|sender_staff_id| format!("group:{conversation_id}:{sender_staff_id}")),
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
    Ok((sender_id, dingtalk_user_id))
}

#[async_trait]
impl LifecycleListener for DingTalkCallbackHandler {
    async fn on_connected(&self, client: Arc<DingTalkStream>, websocket_url: &str) {
        let master_session_ids = self.channel.dingtalk_config.master_session_ids();
        for session_id in master_session_ids {
            if session_id
                .settings(&self.channel.dingtalk_config)
                .map(|it| it.show_connected)
                .unwrap_or(false)
            {
                if let Ok(message) = DingtalkChannel::create_robot_messages_actual(
                    &session_id,
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
                ) {
                    let _ = client.send_message(message).await;
                }
            }
        }
    }

    async fn on_disconnected(
        &self,
        client: Arc<DingTalkStream>,
        result: &dingtalk_stream::Result<()>,
    ) {
        let master_session_ids = self.channel.dingtalk_config.master_session_ids();
        for session_id in master_session_ids {
            if session_id
                .settings(&self.channel.dingtalk_config)
                .map(|it| it.show_connected)
                .unwrap_or(false)
            {
                match result {
                    Ok(_) => {
                        if let Ok(message) = DingtalkChannel::create_robot_messages_actual(
                            &session_id,
                            MessageContentText::from("disconnected from dingtalk websocket"),
                        ) {
                            let _ = client.send_message(message).await;
                        }
                    }
                    Err(err) => {
                        if let Ok(message) = DingtalkChannel::create_robot_messages_actual(
                            &session_id,
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
                        ) {
                            let _ = client.send_message(message).await;
                        }
                    }
                }
            }
        }
    }
}
