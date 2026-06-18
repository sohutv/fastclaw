use crate::channels::{
    AgentRespState, Channel, ChannelContext, ChannelMessage, SessionId, spawn_agent_request,
};
use async_trait::async_trait;
use derive_more::{Deref, From};
use log::warn;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

mod config;
use crate::agent::{Agent, AgentRequest};
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

    pub async fn spawn_request<C: Into<A2AChannelClient>>(
        &'static self,
        client: C,
        req: AgentRequest,
    ) -> crate::Result<tokio::task::JoinHandle<()>> {
        let client = Arc::new(client.into());
        let agent = self.context().agent_registry.get(&req.agent_id).await?;
        let mut receiver = spawn_agent_request::apply(&*agent, req).await?;
        let join_handle = tokio::spawn(async move {
            let _ = self
                .handle_agent_message(client, agent, &mut receiver)
                .await;
        });
        Ok(join_handle)
    }
}
#[derive(Clone, Deref, From)]
pub struct A2AChannelClient(Arc<dyn Agent>);

impl From<&Arc<dyn Agent>> for A2AChannelClient {
    fn from(value: &Arc<dyn Agent>) -> Self {
        Self(Arc::clone(value))
    }
}

#[async_trait]
impl Channel for A2AChannel {
    type Client = A2AChannelClient;
    type JoinHandle = ();

    async fn start(&'static self) -> crate::Result<(&'static Self, Self::JoinHandle)> {
        Ok((Box::leak(Box::new(self)), ()))
    }

    fn context(&self) -> &'static ChannelContext {
        self.context
    }

    async fn client(&self) -> crate::Result<Arc<Self::Client>> {
        unreachable!("unsupported")
    }

    async fn spawn_agent_request(
        &'static self,
        _: AgentRequest,
    ) -> crate::Result<tokio::task::JoinHandle<()>> {
        unreachable!("use spawn_request replaced")
    }

    async fn handle_agent_message(
        &self,
        client: Arc<Self::Client>,
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
                                    let _ = client
                                        .context()
                                        .a2a_channel
                                        .spawn_request(
                                            Arc::clone(&message_from),
                                            AgentRequest::new(
                                                &SessionId::master(client.id().deref()),
                                                message_from.id(),
                                                message,
                                            ),
                                        )
                                        .await;
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
