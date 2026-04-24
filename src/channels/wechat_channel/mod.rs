use crate::agent::{Agent, AgentRequest};
use crate::channels::console_cmd::Console;
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use base64::Engine;
use log::{info, warn};
use rig::OneOrMany;
use rig::completion::Message;
use rig::message::{DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use wechat_sdk::client::message::{MessageItem, MessageItemValue, MessageItems, TextItem};
use wechat_sdk::client::{WechatClient, WechatConfig as WechatInnerConfig, message::WechatMessage};

mod config;

mod recv_agent_message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WechatConfig {
    pub session_id: SessionId,
}

pub struct WechatChannel {
    pub ctx: Arc<ChannelContext>,
    pub wechat_config: WechatConfig,
}

impl WechatChannel {
    pub async fn new(
        config: &'static Config,
        workspace: &'static Workspace,
    ) -> crate::Result<Self> {
        Ok(Self {
            ctx: Arc::new(ChannelContext {
                config: config.clone(),
                workspace,
            }),
            wechat_config: config
                .wechat_config
                .clone()
                .ok_or(anyhow!("dingtalk config not found"))?,
        })
    }
}

#[async_trait]
impl Channel for WechatChannel {
    type Client = WechatClient;
    type JoinHandle = tokio::task::JoinHandle<crate::Result<()>>;

    async fn start(
        self,
        agent: Arc<dyn Agent>,
    ) -> crate::Result<(Arc<Self>, Arc<Self::Client>, Self::JoinHandle)> {
        let wechat_config = WechatInnerConfig {
            state_path: self
                .ctx
                .workspace
                .path
                .parent()
                .expect("unexpected workspace path parent")
                .join("wechat"),
            account_id: self.wechat_config.session_id.to_string().into(),
            http_timeout: Default::default(),
            qr_login_timeout: Default::default(),
            http_api_get_updates_timeout: Default::default(),
        };
        let wechat_client = Arc::new(
            WechatClient::new(wechat_config)
                .await?
                .init(async |url| {
                    println!("open url {} and scan qr-code for login", url);
                    Ok(())
                })
                .await?,
        );
        let self_ = Arc::new(self);
        let join_handle = {
            let self_ = Arc::clone(&self_);
            let wechat_client = Arc::clone(&wechat_client);
            tokio::spawn(async move {
                if self_.wechat_config.session_id.settings().show_connected {
                    let _ = wechat_client.send_message("robot connected").await;
                }
                loop {
                    match wechat_client.get_updates().await {
                        Ok(messages) => {
                            if let Some(message) = messages.into_iter().reduce(|mut l, mut r| {
                                let _ = (&mut l.items).append(&mut r.items);
                                l
                            }) {
                                let _ = Arc::clone(&self_)
                                    .handle_wechat_message(
                                        Arc::clone(&agent),
                                        Arc::clone(&wechat_client),
                                        message,
                                    )
                                    .await;
                                continue;
                            }
                        }
                        Err(err) => {
                            warn!("{err}");
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            })
        };
        Ok((self_, wechat_client, join_handle))
    }

    async fn handle_agent_message(
        &self,
        wechat: Arc<WechatClient>,
        receiver: &mut Receiver<ChannelMessage>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        let typing_ticket = wechat.get_config().await.ok();
        while let Some(message) = receiver.recv().await {
            match self
                .handle_agent_message_actual(
                    &wechat,
                    typing_ticket.as_ref(),
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
        if let Some(typing_ticket) = typing_ticket {
            let _ = wechat.send_typing_cannel(&typing_ticket).await;
        }
        Ok(())
    }

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>> {
        Ok(vec![&self.wechat_config.session_id])
    }
}

impl WechatChannel {
    /// ### handle_wechat_message
    /// - wechat-bot 不支持群聊, 所以不会出现未授权的会话
    async fn handle_wechat_message(
        self: Arc<Self>,
        agent: Arc<dyn Agent>,
        wechat_client: Arc<WechatClient>,
        data: WechatMessage,
    ) -> crate::Result<()> {
        // wechat bot 不支持群聊, 所以不会出现未授权的会话
        let WechatMessage {
            message_id, items, ..
        } = data;
        let (cmd, mut user_contents) = {
            let mut cmd = None;
            let mut user_contents = vec![];
            let mut img_idx = 0usize;
            for MessageItem { value, .. } in items.deref() {
                match value {
                    MessageItemValue::Text {
                        text_item: TextItem { text, .. },
                    } => {
                        if text.starts_with('/') {
                            cmd.replace(text.to_string());
                        }
                        if !text.is_empty() {
                            user_contents.push(UserContent::text(text));
                        }
                    }
                    MessageItemValue::Image { image_item, .. } => {
                        let Some(image) = image_item
                            .media
                            .download(&wechat_client.http_client, Some(&image_item.aes_key))
                            .await
                            .ok()
                            .and_then(|buf| image::load_from_memory(&buf).ok())
                        else {
                            warn!("download {} failed", image_item.media.full_url);
                            continue;
                        };
                        let mut image_data = vec![];
                        let mut cursor = Cursor::new(&mut image_data);
                        let Ok(_) = image.write_to(&mut cursor, image::ImageFormat::Png) else {
                            warn!(
                                "convert image to png failed, image-url: {}",
                                image_item.media.full_url
                            );
                            continue;
                        };
                        let filepath = &self
                            .ctx
                            .workspace
                            .downloads_path()
                            .join(format!("{}.png", uuid::Uuid::new_v4()));
                        let Ok(_) = tokio::fs::write(&filepath, &image_data).await else {
                            warn!(
                                "save image failed, image-url: {}",
                                image_item.media.full_url
                            );
                            continue;
                        };
                        img_idx += 1;
                        user_contents.push(UserContent::Image(Image {
                            data: DocumentSourceKind::Base64(
                                base64::engine::general_purpose::STANDARD.encode(&image_data),
                            ),
                            media_type: Some(ImageMediaType::PNG),
                            detail: Some(ImageDetail::Auto),
                            additional_params: None,
                        }));
                        user_contents.push(UserContent::Text(
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
                    MessageItemValue::File { file_item, .. } => {
                        let Some(file_data) = file_item
                            .media
                            .download(&wechat_client.http_client, None)
                            .await
                            .ok()
                        else {
                            warn!("download {} failed", file_item.media.full_url);
                            continue;
                        };
                        let filepath = &self.ctx.workspace.downloads_path().join(format!(
                            "{}_{}",
                            uuid::Uuid::new_v4(),
                            file_item.file_name
                        ));
                        let Ok(_) = tokio::fs::write(&filepath, &file_data).await else {
                            warn!(
                                "save file failed, file-name: {}. file-url: {}",
                                file_item.file_name, file_item.media.full_url
                            );
                            continue;
                        };
                        user_contents.push(UserContent::Text(
                            format!(
                                r#"
- **filepath of input file**: {}
                "#,
                                filepath.display()
                            )
                            .into(),
                        ));
                    }
                    MessageItemValue::Voice { voice_item, .. } => {
                        if let Some(text) = voice_item.text.as_ref().filter(|it| !it.is_empty()) {
                            user_contents.push(UserContent::text(text));
                        }
                    }
                    _ => continue,
                }
            }
            (cmd, user_contents)
        };
        if let Some(cmd_val) = &cmd {
            match Console::handle_console_cmd(
                &self.ctx,
                &cmd_val,
                &agent,
                &self.wechat_config.session_id,
            )
            .await
            {
                Ok(mut receiver) => {
                    let self_ = Arc::clone(&self);
                    let client = Arc::clone(&wechat_client);
                    let _ = tokio::spawn(async move {
                        let _ = self_.handle_agent_message(client, &mut receiver).await;
                    });
                    return Ok(());
                }
                Err(_) => {}
            }
        }
        let user_content = if user_contents.is_empty() {
            None
        } else {
            if user_contents.len() == 1 {
                user_contents.pop().map(|it| OneOrMany::one(it))
            } else {
                OneOrMany::many(user_contents).ok()
            }
        };
        let Some(user_content) = user_content else {
            return Ok(());
        };
        let msg_id = message_id.clone();
        info!("Submit task to agent, msg_id: {}", msg_id);
        match Arc::clone(&self)
            .submit_agent_task(
                Arc::clone(&wechat_client),
                Arc::clone(&agent),
                None,
                AgentRequest {
                    id: msg_id.to_string().into(),
                    session_id: self.wechat_config.session_id.clone(),
                    message: Message::User {
                        content: user_content,
                    },
                },
            )
            .await
        {
            Ok(_) => {
                let msg = format!("Submit agent task ok, msg_id: {}", msg_id);
                info!("{msg}");
                Ok(())
            }
            Err(err) => {
                warn!("Agent run failed, msg_id: {}, error: {}", msg_id, err);
                Ok(())
            }
        }
    }
}

impl WechatChannel {
    fn create_robot_messages<Content: Into<MessageItems>>(
        session_id: &SessionId,
        _: &ChannelContext,
        content: Content,
    ) -> crate::Result<WechatRobotMessage> {
        let message = match &session_id {
            SessionId::Master { .. } | SessionId::Anonymous { .. } => WechatRobotMessage {
                content: content.into(),
            },
            SessionId::Group { .. } => {
                unreachable!("send robot message to group is not supported by wechat")
            }
        };
        Ok(message)
    }
}

struct WechatRobotMessage {
    content: MessageItems,
}

impl WechatRobotMessage {
    async fn send(self, wechat: &WechatClient) -> crate::Result<()> {
        let _ = wechat.send_message(self.content).await?;
        Ok(())
    }
}
