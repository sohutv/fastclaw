use crate::agent::{Agent, AgentRequest, RequestId};
use crate::channels::console_cmd::Console;
use crate::channels::{Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use log::{info, warn};
use rig::OneOrMany;
use rig::completion::Message;
use rig::message::UserContent;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use wechat_sdk::account::WechatAccountId;
use wechat_sdk::client::message::{
    MessageItem, MessageItemValue, MessageItems, TextItem, ToUserId,
};
use wechat_sdk::client::{WechatClient, WechatConfig as WechatInnerConfig, message::WechatMessage};

mod config;

mod recv_agent_message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WechatConfig {
    // o9cq808B3iiWivLs-uzgKSmbwtXI@im.wechat
    pub account_id: WechatAccountId,
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
            account_id: self.wechat_config.account_id.clone(),
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
            let ctx = Arc::clone(&self.ctx);
            let session_id = self.wechat_config.session_id.clone();
            tokio::spawn(async move {
                if session_id.settings().show_connected {
                    let _ = wechat_client
                        .send_message(session_id.to_string(), "robot connected", None)
                        .await;
                }
                loop {
                    match wechat_client.get_updates().await {
                        Ok(messages) => {
                            if let Some(message) = messages.into_iter().reduce(|mut l, mut r| {
                                let _ = (&mut l.items).append(&mut r.items);
                                l
                            }) {
                                let _ = Self::handle_wechat_message(
                                    Arc::clone(&ctx),
                                    &session_id,
                                    Arc::clone(&agent),
                                    Arc::clone(&wechat_client),
                                    message,
                                )
                                .await;
                            }
                        }
                        Err(err) => {
                            warn!("{err}");
                        }
                    }
                }
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

    /// ### handle_wechat_message
    /// - wechat-bot 不支持群聊, 所以不会出现未授权的会话
    async fn handle_wechat_message(
        ctx: Arc<ChannelContext>,
        session_id: &SessionId,
        agent: Arc<dyn Agent>,
        wechat_client: Arc<WechatClient>,
        data: WechatMessage,
    ) -> crate::Result<()> {
        // wechat bot 不支持群聊, 所以不会出现未授权的会话
        let WechatMessage {
            message_id, items, ..
        } = data;
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

        let msg_id = message_id.clone();
        info!("Submit task to agent, msg_id: {}", msg_id);
        let task_id = RequestId::default();
        let agent = Arc::clone(&agent);
        match WechatChannel::spawn_agent_task(
            AgentRequest {
                id: task_id.clone(),
                session_id: session_id.clone(),
                message: Message::User {
                    content: user_content,
                },
            },
            move || agent,
            None,
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

impl WechatChannel {
    fn create_robot_messages<Content: Into<MessageItems>>(
        session_id: &SessionId,
        _: &ChannelContext,
        content: Content,
    ) -> crate::Result<WechatRobotMessage> {
        let content = content.into();
        let message = match &session_id {
            SessionId::Master { .. } | SessionId::Anonymous { .. } => WechatRobotMessage {
                to_user_id: session_id.to_string().into(),
                content,
            },
            SessionId::Group { .. } => {
                unreachable!("send robot message to group is not supported by wechat")
            }
        };
        Ok(message)
    }
}

struct WechatRobotMessage {
    to_user_id: ToUserId,
    content: MessageItems,
}

impl WechatRobotMessage {
    async fn send(self, wechat: &WechatClient) -> crate::Result<()> {
        let _ = wechat
            .send_message(self.to_user_id, self.content, None)
            .await?;
        Ok(())
    }
}
