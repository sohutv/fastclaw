use crate::agent::MainAgent;
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use anyhow::anyhow;
use async_trait::async_trait;
use log::warn;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use wechat_sdk::client::{WechatClient, WechatConfig as WechatInnerConfig};

mod config;
pub use config::WechatConfig;
mod handle_input_message;
mod recv_agent_message;

pub struct WechatChannel {
    context: &'static ChannelContext,
    pub wechat_config: WechatConfig,
    pub agent: Arc<MainAgent>,
}

impl WechatChannel {
    pub async fn new(
        context: &'static ChannelContext,
        agent: &Arc<MainAgent>,
    ) -> crate::Result<Self> {
        Ok(Self {
            context,
            wechat_config: context
                .config
                .wechat_config
                .clone()
                .ok_or(anyhow!("dingtalk config not found"))?,
            agent: Arc::clone(agent),
        })
    }
}

#[async_trait]
impl Channel for WechatChannel {
    type Client = WechatClient;
    type JoinHandle = tokio::task::JoinHandle<crate::Result<()>>;

    async fn start(
        &'static self,
    ) -> crate::Result<(&'static Self, Arc<Self::Client>, Self::JoinHandle)> {
        let wechat_config = WechatInnerConfig {
            state_path: self
                .context
                .workspace
                .path
                .parent()
                .expect("unexpected workspace path parent")
                .join("wechat"),
            account_id: self.wechat_config.session_id().to_string().into(),
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
                if self.wechat_config.session_config.settings.show_connected {
                    let _ = wechat_client.send_message("robot connected").await;
                }
                loop {
                    match wechat_client.get_updates().await {
                        Ok(messages) => {
                            if let Some(message) = messages.into_iter().reduce(|mut l, mut r| {
                                let _ = (&mut l.items).append(&mut r.items);
                                l
                            }) {
                                let _ = self
                                    .handle_input_message(Arc::clone(&wechat_client), message)
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
        Ok((self, wechat_client, join_handle))
    }

    fn context(&self) -> &'static ChannelContext {
        self.context
    }

    async fn handle_agent_message(
        &self,
        wechat: Arc<WechatClient>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        let typing_ticket = wechat.get_config().await.ok();
        while let Some(message_result) = receiver.recv().await {
            match message_result {
                Ok(message) => {
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
                Err(err) => {
                    warn!("recv error channel message: {err}");
                }
            }
        }
        if let Some(typing_ticket) = typing_ticket {
            let _ = wechat.send_typing_cannel(&typing_ticket).await;
        }
        Ok(())
    }

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>> {
        Ok(vec![&self.wechat_config.session_id()])
    }
}
