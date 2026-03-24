use crate::agent::{
    AgentContext, AgentName, AgentRequest, AgentResponse, HistoryManager, LlmAgentSupplier,
    Workspace,
};
use crate::channels::ChannelMessage;
use crate::config::Config;
use crate::model_provider::{ModelName, ModelProvider, ModelSettings};
use async_trait::async_trait;
use itertools::Itertools;
use log::warn;
use rig::OneOrMany;
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::completion::Message;
use rig::message::UserContent;
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct LlmAgent<C>
where
    C: CompletionClient,
{
    name: AgentName,
    ctx: Arc<AgentContext>,
    model_settings: ModelSettings,
    agent: Agent<C::CompletionModel>,
}

#[async_trait]
impl<C, P> LlmAgentSupplier for P
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    type A = LlmAgent<C>;

    async fn create_agent<N: Into<AgentName> + Send>(
        &self,
        name: N,
        config: &'static Config,
        model: ModelName,
        history_manager: &Arc<RwLock<dyn HistoryManager>>,
        workspace: &'static Workspace,
    ) -> crate::Result<Self::A> {
        Ok(LlmAgent::new(
            name.into(),
            config,
            self.clone(),
            model,
            Arc::clone(history_manager),
            workspace,
        )
        .await?)
    }
}

impl<C> LlmAgent<C>
where
    C: CompletionClient,
{
    async fn new<P>(
        name: AgentName,
        config: &'static Config,
        model_provider: P,
        model: ModelName,
        history_manager: Arc<RwLock<dyn HistoryManager>>,
        workspace: &'static Workspace,
    ) -> crate::Result<Self>
    where
        P: ModelProvider<Client = C>,
    {
        let ctx = Arc::new(AgentContext {
            config,
            workspace,
            history_manager,
        });
        let model_settings = *model_provider
            .model_settings(&model)
            .expect("model settings not found");
        let agent =
            Self::create_agent(&model_provider, &model, &model_settings, Arc::clone(&ctx)).await?;
        Ok(Self {
            name: name.into(),
            ctx,
            model_settings,
            agent,
        })
    }

    async fn create_agent<P>(
        provider: &P,
        model: &ModelName,
        model_settings: &ModelSettings,
        ctx: Arc<AgentContext>,
    ) -> crate::Result<Agent<C::CompletionModel>>
    where
        P: ModelProvider<Client = C>,
    {
        let model_client = provider.completion_client()?;
        let ModelSettings {
            temperature, tool, ..
        } = model_settings;
        let agent = model_client
            .agent(model.as_str())
            .preamble(&*super::prompt::PromptSection::Identity.build(&ctx).await?)
            .tools(crate::tools::FunctionTool::required_tools(Arc::clone(
                &ctx,
            ))?)
            .temperature(**temperature)
            .default_max_turns(256)
            .build();
        Ok(agent)
    }
}
impl<C> super::Agent for LlmAgent<C>
where
    C: CompletionClient + 'static + Send + Sync,
{
    fn run(
        self: Self,
        channel_message_sender: Sender<ChannelMessage>,
    ) -> crate::Result<(JoinHandle<()>, Sender<AgentRequest>)> {
        let (request_sender, mut request_receiver) = {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            (tx, rx)
        };
        let handle = tokio::task::spawn(async move {
            let channel_message_sender = channel_message_sender;
            while let Some(message) = request_receiver.recv().await {
                self.handle_message(message, channel_message_sender.clone())
                    .await;
            }
        });
        Ok((handle, request_sender))
    }
}

impl<C> LlmAgent<C>
where
    C: CompletionClient + 'static + Send + Sync,
{
    async fn handle_message(
        &self,
        request: AgentRequest,
        channel_message_sender: Sender<ChannelMessage>,
    ) {
        let Some(ref request @ AgentRequest { ref session_id, .. }) = self.request_filter(request)
        else {
            return;
        };
        let _ = channel_message_sender
            .send(ChannelMessage {
                session_id: session_id.clone(),
                message: AgentResponse::Start,
            })
            .await;
        let history = {
            let mgr = self.ctx.history_manager.read().await;
            mgr.load(session_id, &self.name).await.unwrap_or_default()
        };
        let mut stream = self.agent.stream_chat(request.clone(), history).await;
        while let Some(result) = stream.next().await {
            let response = match result {
                Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                    StreamedAssistantContent::ReasoningDelta { reasoning, .. } => {
                        Some(AgentResponse::ReasoningStream(
                            rig::completion::message::Reasoning::new(&reasoning),
                        ))
                    }
                    StreamedAssistantContent::Text(text) => Some(AgentResponse::MessageStream(
                        Message::assistant(text.text()),
                    )),
                    StreamedAssistantContent::ToolCall { tool_call, .. } => {
                        Some(AgentResponse::ToolCall(tool_call))
                    }
                    _ => None,
                },
                Ok(MultiTurnStreamItem::StreamUserItem(_)) => None,
                Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                    let usage = final_resp.usage();
                    let history = final_resp.history().expect("unexpected empty history!!!");
                    {
                        let mut mgr = self.ctx.history_manager.write().await;
                        match mgr
                            .store(&request.session_id, &self.name, &usage, history)
                            .await
                        {
                            Ok(_) => {}
                            Err(err) => {
                                warn!(
                                    "Store history failed, session_id: {}, agent: {}, err: {}",
                                    session_id, self.name, err
                                );
                            }
                        }
                    }
                    Some(AgentResponse::Final(usage))
                }
                Ok(_) => None,
                Err(err) => Some(AgentResponse::Error(err.to_string())),
            };
            if let Some(message) = response {
                let _ = channel_message_sender
                    .send(ChannelMessage {
                        session_id: session_id.clone(),
                        message,
                    })
                    .await;
            }
        }
    }

    fn request_filter(&self, request: AgentRequest) -> Option<AgentRequest> {
        if let Message::User { content, .. } = request.message {
            let mut vec = content
                .into_iter()
                .filter(|item| match item {
                    UserContent::Image(_) => self.model_settings.vision,
                    UserContent::Audio(_) => self.model_settings.audio,
                    UserContent::Video(_) => self.model_settings.video,
                    UserContent::Document(_) => self.model_settings.document,
                    _ => true,
                })
                .collect_vec();
            match vec.len() {
                0 => None,
                1 => Some(AgentRequest {
                    session_id: request.session_id,
                    message: Message::User {
                        content: OneOrMany::one(vec.remove(0)),
                    },
                }),
                2.. => OneOrMany::many(vec).ok().map(|content| AgentRequest {
                    session_id: request.session_id,
                    message: Message::User { content },
                }),
            }
        } else {
            Some(request)
        }
    }
}
