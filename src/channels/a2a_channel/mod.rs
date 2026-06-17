use crate::channels::{AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId};
use async_trait::async_trait;
use log::warn;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

mod config;
pub use config::A2AChannelConfig;

mod recv_agent_message;
pub struct A2AChannel {
    pub context: &'static ChannelContext,
    pub config: A2AChannelConfig,
}

impl A2AChannel {
    pub async fn new(channel_context: &'static ChannelContext) -> crate::Result<Self> {
        Ok(A2AChannel {
            context: channel_context,
            config: channel_context.config.a2a_channel.clone(),
        })
    }
}

#[async_trait]
impl Channel for A2AChannel {
    type Client = ();
    type JoinHandle = ();

    async fn start(&'static self) -> crate::Result<(&'static Self, Arc<Self::Client>, Self::JoinHandle)> {
        Ok((Box::leak(Box::new(self)), Default::default(), ()))
    }

    fn context(&self) -> &'static ChannelContext {
        self.context
    }

    async fn handle_agent_message(
        &self,
        _: Arc<Self::Client>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message_result) = receiver.recv().await {
            match message_result {
                Ok(message) => {
                    match self
                        .handle_agent_message_actual(&message, state, &mut buff)
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
        Ok(vec![])
    }
}
