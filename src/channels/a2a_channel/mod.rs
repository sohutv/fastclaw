use crate::channels::{
    AgentRespState, Channel, ChannelContext, ChannelMessage, ChannelNotifier, SessionId,
};
use async_trait::async_trait;
use log::warn;
use tokio::sync::mpsc::Receiver;

mod config;
pub use config::A2AChannelConfig;

mod recv_agent_message;
pub struct A2AChannel {
    pub context: &'static ChannelContext,
    pub config: A2AChannelConfig,
    pub client: (),
}

impl A2AChannel {
    pub async fn new(channel_context: &'static ChannelContext) -> crate::Result<Self> {
        Ok(A2AChannel {
            context: channel_context,
            config: channel_context.config.a2a_channel.clone(),
            client: Default::default(),
        })
    }
}

#[async_trait]
impl Channel for A2AChannel {
    type Client = ();
    type JoinHandle = ();

    async fn start(
        &'static self,
    ) -> crate::Result<(&'static Self, ChannelNotifier, Self::JoinHandle)> {
        let (tx, _rx) = tokio::sync::mpsc::channel::<super::Notify>(32);
        Ok((Box::leak(Box::new(self)), tx.into(), ()))
    }

    fn context(&self) -> &'static ChannelContext {
        self.context
    }

    async fn client(&self) -> crate::Result<()> {
        Ok(self.client)
    }

    async fn handle_agent_message(
        &self,
        client: &Self::Client,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff: Vec<String> = vec![];
        while let Some(message_result) = receiver.recv().await {
            match message_result {
                Ok(message) => {
                    match self
                        .handle_agent_message_actual(&client, &message, state, &mut buff)
                        .await
                    {
                        Ok((next, message)) => {
                            if let Some(message) = message {
                                print!("{message}");
                            }
                            match next {
                                AgentRespState::Final => {
                                    state = AgentRespState::Wait;
                                    buff.clear();
                                }
                                next => state = next,
                            }
                        }
                        Err(_) => {
                            state = AgentRespState::Wait;
                            buff.clear();
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
