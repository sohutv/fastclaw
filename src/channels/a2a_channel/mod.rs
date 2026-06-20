use crate::channels::{
    AgentRespState, Channel, ChannelContext, ChannelMessage, ChannelNotifier, SessionId,
};
use async_trait::async_trait;
use log::warn;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

mod config;
use crate::agent::Agent;
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
        message_from: Arc<dyn Agent>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff: Vec<String> = vec![];
        let mut received = vec![];
        while let Some(message_result) = receiver.recv().await {
            match message_result {
                Ok(message) => {
                    match self
                        .handle_agent_message_actual(&client, &message, state, &mut buff)
                        .await
                    {
                        Ok((next, message)) => {
                            if let Some(message) = message {
                                received.push(message);
                            }
                            match next {
                                AgentRespState::Final => {
                                    state = AgentRespState::Wait;
                                    buff.clear();
                                    let message = {
                                        let message = received.join("\n");
                                        received.clear();
                                        message
                                    };
                                    println!(
                                        r#"
from: =========={}==========
{}
"#,
                                        message_from.id(),
                                        message
                                    );
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
