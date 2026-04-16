use crate::agent::{Agent, AgentRequest, RequestId};
use crate::channels::console_cmd::Console;
use crate::channels::dingtalk_channel::{DingTalkConfig, DingtalkChannel};
use crate::channels::{ChannelContext, SessionId, UserId, session_id};
use async_trait::async_trait;
use base64::Engine;
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
use itertools::Itertools;
use log::{info, warn};
use rig::OneOrMany;
use rig::completion::Message;
use rig::message::{DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent};
use std::io::Cursor;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

#[allow(unused)]
pub(super) struct DingTalkCallbackHandler {
    pub(super) ctx: Arc<ChannelContext>,
    pub(super) config: DingTalkConfig,
    pub(super) dingtalk_bot_topic: MessageTopic,
    pub(super) agent: Arc<dyn Agent>,
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
                let downloads_dir = self.ctx.workspace.downloads_path().to_path_buf();
                match picture.fetch(&dingtalk_client, downloads_dir).await {
                    Ok((filepath, image)) => {
                        (None, None, Some(vec![(1usize, filepath, image)]), None)
                    }
                    Err(e) => (None, Some(format!("下载图片失败, {}", e)), None, None),
                }
            }
            MessagePayload::File { content } => {
                let downloads_dir = self.ctx.workspace.downloads_path().to_path_buf();
                match content.fetch(&dingtalk_client, downloads_dir).await {
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
                let downloads_dir = self.ctx.workspace.downloads_path().to_path_buf();
                let mut texts = vec![];
                let mut pictures = vec![];
                let mut img_idx = 0;
                for content in content.iter() {
                    match content {
                        RichTextItem::Text(text) => {
                            texts.push(text.to_string());
                        }
                        RichTextItem::Picture(picture) => {
                            match picture.fetch(&dingtalk_client, downloads_dir.clone()).await {
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
        let line = if let Some(cmd_val) = &cmd {
            match Console::handle_console_cmd(&self.ctx, &cmd_val, &self.agent, &session_id).await {
                Ok(mut receiver) => {
                    let client = Arc::clone(&dingtalk_client);
                    let ctx = Arc::clone(&self.ctx);
                    let _ = tokio::spawn(async move {
                        let _ =
                            DingtalkChannel::recv_agent_message(client, &ctx, &mut receiver).await;
                    });
                    return Ok(HandlerResp::Text("cmd submitted".to_string()));
                }
                Err(_) => {}
            }
            cmd
        } else {
            line
        };
        let prompts = vec![UserContent::text(line.as_deref().unwrap_or_default())];

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
- **filepath of the {}-th input image**: {}
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
- **filepath of input file**: {}
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

        let addi_system_prompt = match &session_id {
            SessionId::Master {
                val: session_id, ..
            } => format!(
                "- **Attention current session_id**: {}. You are speaking to your owner",
                session_id
            ),
            SessionId::Anonymous {
                val: session_id, ..
            } => format!(
                "- **Attention current session_id**: {}. You are currently not interacting with your owner. Please stay vigilant.",
                session_id
            ),
            SessionId::Group {
                val:
                    session_id::Group {
                        session_id,
                        name: group_name,
                        user_id,
                        ..
                    },
                ..
            } => match user_id {
                UserId::Master(_) => format!(
                    "- **Attention current session_id**: {}. This session is a group session, group_id: {}, group_name: {}. You are speaking to your owner",
                    session_id,
                    session_id,
                    group_name.as_deref().unwrap_or("..no provided.."),
                ),
                UserId::Anonymous(_) => format!(
                    "- **Attention current session_id**: {}. This session is a group session, group_id: {}, group_name: {}. You are currently not interacting with your owner. Please stay vigilant.",
                    session_id,
                    session_id,
                    group_name.as_deref().unwrap_or("..no provided.."),
                ),
            },
        };

        let msg_id = msg_id.clone();
        info!("Submit task to agent, msg_id: {}", msg_id);
        let task_id = RequestId::default();
        let agent = Arc::clone(&self.agent);
        match DingtalkChannel::spawn_agent_task(
            AgentRequest {
                id: task_id.clone(),
                session_id,
                message: Message::User {
                    content: user_content,
                },
            },
            move || agent,
            Some(addi_system_prompt),
        )
        .await
        {
            Ok(receiver) => {
                let msg = format!(
                    "Submit agent task ok, msg_id: {}, task_id: {}",
                    msg_id, task_id
                );
                info!("{msg}");
                {
                    let mut receiver = receiver;
                    let client = Arc::clone(&dingtalk_client);
                    let ctx = Arc::clone(&self.ctx);
                    let _ = tokio::spawn(async move {
                        let _ =
                            DingtalkChannel::recv_agent_message(client, &ctx, &mut receiver).await;
                    });
                }
                Ok(HandlerResp::Text(msg))
            }
            Err(err) => {
                warn!(
                    "Agent run failed, msg_id: {}, task_id: {}, error: {}",
                    msg_id, task_id, err
                );
                Ok(HandlerResp::Text(format!(
                    "Submit agent task failed, msg_id: {}, task_id: {}",
                    msg_id, task_id
                )))
            }
        }
    }

    fn topic(&self) -> &MessageTopic {
        &self.dingtalk_bot_topic
    }
}

#[async_trait]
impl LifecycleListener for DingTalkCallbackHandler {
    async fn on_connected(&self, client: Arc<DingTalkStream>, websocket_url: &str) {
        let master_session_ids = self.config.master_session_ids();
        for session_id in master_session_ids {
            if !session_id.settings().show_connected {
                continue;
            }
            let Some(message) = DingtalkChannel::create_robot_messages(
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

    async fn on_disconnected(
        &self,
        client: Arc<DingTalkStream>,
        result: &dingtalk_stream::Result<()>,
    ) {
        let master_session_ids = self.config.master_session_ids();
        for session_id in master_session_ids {
            if !session_id.settings().show_disconnected {
                continue;
            }
            match result {
                Ok(_) => {
                    let Some(message) = DingtalkChannel::create_robot_messages(
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
                    let Some(message) = DingtalkChannel::create_robot_messages(
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
