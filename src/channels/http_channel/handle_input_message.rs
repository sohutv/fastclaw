use crate::agent::{Agent, AgentRequest};
use crate::channels::console_cmd::Console;
use crate::channels::http_channel::{CameraFrame, Client, HttpChannel};
use crate::channels::http_channel::{HttpReqMessage, Payload};
use crate::channels::{Channel, SessionId};
use base64::Engine;
use log::{info, warn};
use rig::OneOrMany;
use rig::completion::Message;
use rig::message::{DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent};
use std::ops::Deref;
use std::sync::Arc;
use zerocopy::IntoBytes;

impl HttpChannel {
    /// ### handle_input_message
    pub(super) async fn handle_input_message(
        self: Arc<Self>,
        agent: Arc<dyn Agent>,
        session_id: SessionId,
        client: Arc<Client>,
        data: HttpReqMessage,
    ) -> crate::Result<()> {
        let HttpReqMessage {
            message_id,
            payloads,
            ..
        } = &data;
        let (cmd, mut user_contents) = {
            let mut cmd = None;
            let mut user_contents = vec![];
            let mut img_idx = 0usize;
            for payload in payloads {
                match payload {
                    Payload::Text(text) => {
                        if text.starts_with('/') {
                            cmd.replace(text.to_string());
                        }
                        if !text.is_empty() {
                            user_contents.push(UserContent::text(text.deref()));
                        }
                    }
                    Payload::Json(json) => match serde_json::to_string(json) {
                        Ok(text) => {
                            if !text.is_empty() {
                                user_contents.push(UserContent::text(text));
                            }
                        }
                        Err(err) => {
                            warn!("write json to text failed, err: {err}");
                            continue;
                        }
                    },
                    Payload::Image(image) => {
                        let extension = match image.extension() {
                            Ok(val) => val,
                            Err(err) => {
                                warn!("save image failed, err: {err}",);
                                continue;
                            }
                        };
                        let filepath = &self.ctx.workspace.downloads_path().join(format!(
                            "{}.{}",
                            uuid::Uuid::new_v4(),
                            extension
                        ));
                        match tokio::fs::write(&filepath, image.content.as_bytes()).await {
                            Ok(_) => {}
                            Err(err) => {
                                warn!("save image failed, err: {err}",);
                                continue;
                            }
                        }
                        img_idx += 1;
                        user_contents.push(UserContent::Image(Image {
                            data: DocumentSourceKind::Base64(
                                base64::engine::general_purpose::STANDARD.encode(&image.content),
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
                    Payload::CameraFrame(CameraFrame { meta, image }) => {
                        img_idx += 1;
                        user_contents.push(UserContent::Image(Image {
                            data: DocumentSourceKind::Base64(
                                base64::engine::general_purpose::STANDARD.encode(&image.content),
                            ),
                            media_type: Some(ImageMediaType::PNG),
                            detail: Some(ImageDetail::Auto),
                            additional_params: None,
                        }));
                        match serde_json::to_string(meta) {
                            Ok(text) => {
                                if !text.is_empty() {
                                    user_contents.push(UserContent::Text(
                                        format!(
                                            "- **camera frame meta of the {}-th input image**: {}",
                                            img_idx, text
                                        )
                                        .into(),
                                    ));
                                }
                            }
                            Err(err) => {
                                warn!("write camera frame meta to text failed, err: {err}");
                                continue;
                            }
                        }
                    }
                }
            }
            (cmd, user_contents)
        };
        if let Some(cmd_val) = &cmd {
            match Console::handle_console_cmd(&self.ctx, &cmd_val, &agent, &session_id).await {
                Ok(mut receiver) => {
                    let self_ = Arc::clone(&self);
                    let client = Arc::clone(&client);
                    let _ = tokio::spawn(async move {
                        let _ = self_
                            .handle_agent_message(client, Some(data), &mut receiver)
                            .await;
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
                Arc::clone(&client),
                Arc::clone(&agent),
                None,
                Some(data),
                AgentRequest {
                    id: msg_id.to_string().into(),
                    session_id: session_id.clone(),
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
