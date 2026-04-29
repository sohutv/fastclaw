use crate::agent::{Agent, AgentRequest};
use crate::channels::Channel;
use crate::channels::console_cmd::Console;
use crate::channels::wechat_channel::WechatChannel;
use base64::Engine;
use log::{info, warn};
use rig::OneOrMany;
use rig::completion::Message;
use rig::message::{DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent};
use std::io::Cursor;
use std::ops::Deref;
use std::sync::Arc;
use wechat_sdk::client::WechatClient;
use wechat_sdk::client::message::{MessageItem, MessageItemValue, TextItem, WechatMessage};

impl WechatChannel {
    /// ### handle_wechat_message
    /// - wechat-bot 不支持群聊, 所以不会出现未授权的会话
    pub(super) async fn handle_input_message(
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
                            warn!("download image {} failed", image_item.media.full_url);
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
                                "- **filepath of the {}-th input image**: {}",
                                img_idx,
                                filepath.display()
                            )
                            .into(),
                        ))
                    }
                    MessageItemValue::Video { video_item, .. } => {
                        let Some(file_data) = video_item
                            .media
                            .download(&wechat_client.http_client, None)
                            .await
                            .ok()
                        else {
                            warn!("download video {} failed", video_item.media.full_url);
                            continue;
                        };

                        let filepath = &self
                            .ctx
                            .workspace
                            .downloads_path()
                            .join(format!("{}.mp4", uuid::Uuid::new_v4(),));
                        let Ok(_) = tokio::fs::write(&filepath, &file_data).await else {
                            warn!("save video failed, file-url: {}", video_item.media.full_url);
                            continue;
                        };
                        user_contents.push(UserContent::Text(
                            format!("- **filepath of input video**: {}", filepath.display()).into(),
                        ));
                    }
                    MessageItemValue::File { file_item, .. } => {
                        let Some(file_data) = file_item
                            .media
                            .download(&wechat_client.http_client, None)
                            .await
                            .ok()
                        else {
                            warn!("download file {} failed", file_item.media.full_url);
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
                            format!("- **filepath of input file**: {}", filepath.display()).into(),
                        ));
                    }
                    MessageItemValue::Voice { voice_item, .. } => {
                        if let Some(text) = voice_item.text.as_ref().filter(|it| !it.is_empty()) {
                            user_contents.push(UserContent::text(text));
                        }
                        let Some(file_data) = voice_item
                            .media
                            .download(&wechat_client.http_client, None)
                            .await
                            .ok()
                        else {
                            warn!("download voice {} failed", voice_item.media.full_url);
                            continue;
                        };
                        let filepath = &self
                            .ctx
                            .workspace
                            .downloads_path()
                            .join(format!("{}.mp4", uuid::Uuid::new_v4(),));
                        let Ok(_) = tokio::fs::write(&filepath, &file_data).await else {
                            warn!("save voice failed, file-url: {}", voice_item.media.full_url);
                            continue;
                        };
                        user_contents.push(UserContent::Text(
                            format!("- **filepath of input voice**: {}", filepath.display()).into(),
                        ));
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
