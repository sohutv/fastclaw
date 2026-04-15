use crate::agent::{Agent, AgentRequest, RequestId};
use crate::channels::console_cmd::Console;
use crate::channels::dingtalk_channel::DingtalkChannel;
use crate::channels::{Channel, ChannelContext, SessionId, UserId, session_id, ChannelMessage};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use base64::Engine;
use dingtalk_stream::frames::down_message::callback_message::{
    Conversation, MessageData, MessagePayload, RichTextItem,
};
use dingtalk_stream::frames::up_message::MessageContent;
use dingtalk_stream::frames::up_message::callback_message::WebhookMessage;
use dingtalk_stream::handlers::ErrorCode;
use itertools::Itertools;
use log::{info, warn};
use rig::OneOrMany;
use rig::completion::Message;
use rig::message::{DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent};
use rig::providers::anthropic::decoders::sse::iter_sse_messages;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use wechat_sdk::account::WechatAccountId;
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
    type Output = (
        Arc<WechatClient>,
        tokio::task::JoinHandle<crate::Result<()>>,
    );

    async fn start(self, agent: Arc<dyn Agent>) -> crate::Result<Self::Output> {
        let wechat_config = WechatInnerConfig {
            state_path: self
                .ctx
                .workspace
                .path
                .parent()
                .expect("unexpected workspace path parent")
                .join("wechat"),
            account_id: WechatAccountId::from_str(&self.wechat_config.session_id)?,
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
        let join_handle = {
            let wechat_client = Arc::clone(&wechat_client);
            tokio::spawn(async move {
                loop {
                    let messages = wechat_client.get_updates().await?;
                }
                Ok(())
            })
        };

        Ok((wechat_client, join_handle))
    }
}

impl WechatChannel {

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

    async fn handle_wechat_message(
        ctx: Arc<ChannelContext>,
        config: &WechatConfig,
        inner_config: &WechatInnerConfig,
        session_id: SessionId,
        agent: Arc<dyn Agent>,
        wechat_client: Arc<WechatClient>,
        data: WechatMessage,
    ) -> crate::Result<()> {
        let WechatMessage {
            message_id,
            from_user_id,
            items,
            ..
        } = data;
        let sender_id = from_user_id.deref();
        if !session_id.deref().eq_ignore_ascii_case(&sender_id) {
            let _ = wechat_client
                .send_message(
                    WechatMessage::new(inner_config, &from_user_id, "talking is forbidden").await?,
                )
                .await?;
            return Ok(());
        }

        let (cmd, line) = {
            let mut cmd = None;
            let mut lines = vec![];
            for MessageItem { value, .. } in items.deref() {
                match value {
                    MessageItemValue::Text {
                        text_item: TextItem { text, .. },
                    } => {
                        if text.starts_with('/') {
                            cmd.replace(text.to_string());
                        } else {
                            if !text.is_empty() {
                                lines.push(text.clone());
                            }
                        }
                    }
                    _ => continue,
                }
            }
            (cmd, Some(lines.join("\n")))
        };
        let line = if let Some(cmd_val) = &cmd {
            match Console::handle_console_cmd(&ctx, &cmd_val, &agent, &session_id).await {
                Ok(mut receiver) => {
                    let client = Arc::clone(&wechat_client);
                    let ctx = Arc::clone(&ctx);
                    let _ = tokio::spawn(async move {
                        let _ =
                            WechatChannel::recv_agent_message(client, &ctx, &mut receiver).await;
                    });
                    return Ok(());
                }
                Err(_) => {}
            }
            cmd
        } else {
            line
        };
        let prompts = vec![UserContent::text(line.as_deref().unwrap_or_default())];

        let mut user_content = Vec::<UserContent>::new();
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
            return Ok(());
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

        let msg_id = message_id.clone();
        info!("Submit task to agent, msg_id: {}", msg_id);
        let task_id = RequestId::default();
        let agent = Arc::clone(&agent);
        match WechatChannel::spawn_agent_task(
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
                    let client = Arc::clone(&wechat_client);
                    let ctx = Arc::clone(&ctx);
                    let _ = tokio::spawn(async move {
                        let _ =
                            WechatChannel::recv_agent_message(client, &ctx, &mut receiver).await;
                    });
                }
                Ok(())
            }
            Err(err) => {
                warn!(
                    "Agent run failed, msg_id: {}, task_id: {}, error: {}",
                    msg_id, task_id, err
                );
                Ok(())
            }
        }
    }
}
