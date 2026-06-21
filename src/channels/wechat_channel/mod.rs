use crate::agent::{Agent, MainAgent};
use crate::channels::{
    AgentRespState, Channel, ChannelContext, ChannelMessage, ChannelNotifier, Notify, SessionId,
};
use anyhow::anyhow;
use async_trait::async_trait;
use log::warn;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use wechat_sdk::client::message::MessageItems;
use wechat_sdk::client::{WechatClient, WechatConfig as WechatInnerConfig};

mod config;
pub use config::WechatConfig;

mod handle_input_message;
mod recv_agent_message;

pub struct WechatChannel {
    context: &'static ChannelContext,
    pub wechat_config: &'static WechatConfig,
    pub wechat_client: Arc<RwLock<Option<Arc<WechatClient>>>>,
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
                .as_ref()
                .ok_or(anyhow!("dingtalk config not found"))?,
            wechat_client: Default::default(),
            agent: Arc::clone(agent),
        })
    }
}

#[async_trait]
impl Channel for WechatChannel {
    type Client = Arc<WechatClient>;
    type JoinHandle = tokio::task::JoinHandle<crate::Result<()>>;

    async fn start(
        &'static self,
    ) -> crate::Result<(&'static Self, ChannelNotifier, Self::JoinHandle)> {
        let mut guard = self.wechat_client.write().await;
        if guard.is_some() {
            return Err(anyhow!("channel had been already started!!!"));
        }
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
        let notifier = {
            let config = self.wechat_config;
            let client = Arc::clone(&wechat_client);
            let (rx, mut tx) = tokio::sync::mpsc::channel(32);
            tokio::spawn(async move {
                while let Some(Notify { content, .. }) = tx.recv().await {
                    let session_id = config.session_id();
                    if session_id
                        .settings(config)
                        .map(|it| it.show_connected)
                        .unwrap_or(false)
                    {
                        if let Ok(message) =
                            Self::create_robot_messages_actual(&session_id, content)
                        {
                            let _ = message.send(&client).await;
                        }
                    }
                }
            });
            rx.into()
        };
        *guard = Some(wechat_client);
        Ok((self, notifier, join_handle))
    }

    fn context(&self) -> &'static ChannelContext {
        self.context
    }

    async fn client(&self) -> crate::Result<Self::Client> {
        self.wechat_client
            .read()
            .await
            .as_ref()
            .map(|it| Arc::clone(it))
            .ok_or(anyhow!("channel not started"))
    }

    async fn handle_agent_message(
        &self,
        wechat: &Self::Client,
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
                            wechat,
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

impl WechatChannel {
    fn create_robot_messages<Content: Into<MessageItems>>(
        _: &WechatClient,
        _: &dyn Agent,
        session_id: &SessionId,
        _: &ChannelContext,
        content: Content,
    ) -> crate::Result<WechatRobotMessage> {
        Self::create_robot_messages_actual(session_id, content)
    }

    fn create_robot_messages_actual<Content: Into<MessageItems>>(
        session_id: &SessionId,
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
