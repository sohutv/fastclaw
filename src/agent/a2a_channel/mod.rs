use crate::agent::{Agent, AgentRequestPkg};
use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use async_trait::async_trait;
use log::warn;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

mod config;
pub use config::A2AChannelConfig;
mod recv_agent_message;
pub struct A2AChannel {
    pub ctx: Arc<ChannelContext>,
    pub config: A2AChannelConfig,
    pub agent: Arc<dyn Agent>,
}

impl A2AChannel {
    pub fn new(delegated: &Arc<dyn Agent>) -> crate::Result<Self> {
        let context = delegated.context();
        Ok(A2AChannel {
            ctx: Arc::new(ChannelContext {
                config: context.config,
                workspace: context.workspace,
            }),
            config: context.config.a2a_channel.clone(),
            agent: Arc::clone(delegated),
        })
    }
}

#[async_trait]
impl Channel for A2AChannel {
    type Client = ();
    type InboundMessage = AgentRequestPkg;
    type JoinHandle = ();

    async fn start(self) -> crate::Result<(Arc<Self>, Arc<Self::Client>, Self::JoinHandle)> {
        let self_ = Arc::new(self);
        let client = Arc::new(());
        Ok((self_, client, ()))
    }

    fn agent(&self) -> &Arc<dyn Agent> {
        &self.agent
    }

    async fn handle_agent_message(
        &self,
        _: Arc<Self::Client>,
        inbound_message: Option<Self::InboundMessage>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message_result) = receiver.recv().await {
            match message_result {
                Ok(message) => {
                    match self
                        .handle_agent_message_actual(
                            inbound_message.as_ref(),
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
        Ok(())
    }

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>> {
        Ok(vec![&self.config.session_id()])
    }
}
