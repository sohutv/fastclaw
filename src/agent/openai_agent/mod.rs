use crate::agent::{AgentName, Context};
use crate::model_provider::Model;
use crate::model_provider::openai_compatible::OpenaiCompatible;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::message::Message;
use rig::providers::openai::CompletionModel;
use rig::streaming::{StreamedAssistantContent, StreamingCompletion};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio_stream::StreamExt;

pub(crate) struct OpenaiAgent {
    name: AgentName,
    ctx: Arc<Context>,
    provider: OpenaiCompatible,
    model: Model,
    agent: rig::agent::Agent<CompletionModel>,
    history: Vec<Message>,
    channel_receiver: Receiver<Message>,
}

impl OpenaiAgent {
    pub(crate) fn new(
        name: AgentName,
        ctx: Arc<Context>,
        provider: OpenaiCompatible,
        model: Model,
    ) -> crate::Result<super::Agent> {
        let client = provider.completion_client()?;
        let agent = client
            .agent(&*model)
            .temperature(*provider.temperature)
            .build();
        let (channel_sender, channel_receiver) = tokio::sync::mpsc::channel(1);
        Ok(super::Agent::OpenaiAgent {
            delegate: Self {
                name,
                ctx,
                provider,
                model,
                agent,
                history: Default::default(),
                channel_receiver,
            },
            channel_sender,
        })
    }
}

impl OpenaiAgent {
    pub async fn run(&mut self) -> crate::Result<()> {
        loop {
            tokio::select! {
                message = self.channel_receiver.recv() => {
                    if let Some(message) = message{
                        self.handle_message(message).await?;
                    }
                },
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        log::info!("agent {} run exited", self.name);
        Ok(())
    }

    async fn handle_message(&mut self, message: Message) -> crate::Result<()> {
        if let Ok(builder) = self
            .agent
            .stream_completion(message, self.history.clone())
            .await
        {
            if let Ok(mut stream) = builder.stream().await {
                while let Some(chunk) = stream.next().await {
                    if let Ok(chunk) = chunk {
                        match chunk {
                            StreamedAssistantContent::Reasoning(reasoning) => {
                                println!("{:?}", reasoning)
                            }
                            StreamedAssistantContent::Text(text) => {
                                println!("{}", text)
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
