use crate::agent::Agent;
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use log::warn;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use wechat_sdk::client::message::MessageItems;
use wechat_sdk::client::{WechatClient, WechatConfig as WechatInnerConfig};

mod config;
mod handle_input_message;
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
                                    .handle_input_message(
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
