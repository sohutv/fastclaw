use crate::agent::{
    AgentContext, AgentCtlSignal, AgentMessage, AgentMessageSender, AgentName, AgentSignal,
    Workspace,
};
use crate::channels::{ChannelMessage, ChannelMessageReceiver, SessionId};
use crate::config::Config;
use crate::model_provider::{ModelName, ModelProvider, ModelSettings};
use itertools::Itertools;
use log::{error, warn};
use rig::OneOrMany;
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::completion::{Message, Usage};
use rig::message::UserContent;
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;

pub(crate) struct LlmAgent<C, P>
where
    C: CompletionClient,
    P: ModelProvider<C>,
{
    name: AgentName,
    ctx: Arc<AgentContext>,
    model_provider: P,
    model: ModelName,
    model_settings: ModelSettings,
    agent: RwLock<Agent<C::CompletionModel>>,
    history: Vec<Message>,
    usage: Usage,
    pub msg_sender: AgentMessageSender,
    msg_receiver: Receiver<AgentMessage>,
    ctl_signal_receiver: Receiver<AgentCtlSignal>,
}

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient,
    P: ModelProvider<C>,
{
    #[allow(unused)]
    pub(crate) async fn fork(&self) -> crate::Result<(Self, ChannelMessageReceiver)> {
        Self::new(
            self.name.clone(),
            self.ctx.config,
            &self.ctx.workspace.path,
            self.model_provider.clone(),
            self.model.clone(),
        )
        .await
    }
}

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient,
    P: ModelProvider<C>,
{
    pub(crate) async fn new<N: Into<AgentName>, WorkDir: AsRef<Path>>(
        name: N,
        config: &'static Config,
        workdir: WorkDir,
        model_provider: P,
        model: ModelName,
    ) -> crate::Result<(Self, ChannelMessageReceiver)> {
        let (msg_sender, msg_receiver) = tokio::sync::mpsc::channel(1);
        let (ctl_signal_sender, ctl_signal_receiver) = tokio::sync::mpsc::channel(1);
        let (channel_message_sender, channel_message_receiver) = tokio::sync::mpsc::channel(1024);
        let ctx = Arc::new(AgentContext {
            config,
            workspace: Workspace::from(&workdir),
            msg_sender: msg_sender.clone().into(),
            ctl_signal_sender: ctl_signal_sender.into(),
            channel_message_sender: channel_message_sender.clone().into(),
        });
        let model_settings = *model_provider
            .model_settings(&model)
            .expect("model settings not found");
        let agent =
            Self::create_agent(&model_provider, &model, &model_settings, Arc::clone(&ctx)).await?;
        Ok((
            Self {
                name: name.into(),
                ctx,
                model_provider,
                model,
                model_settings,
                agent: RwLock::new(agent),
                history: Default::default(),
                usage: Default::default(),
                msg_sender: msg_sender.into(),
                msg_receiver,
                ctl_signal_receiver,
            },
            channel_message_receiver.into(),
        ))
    }

    async fn create_agent(
        provider: &P,
        model: &ModelName,
        model_settings: &ModelSettings,
        ctx: Arc<AgentContext>,
    ) -> crate::Result<Agent<C::CompletionModel>> {
        let model_client = provider.completion_client()?;
        let ModelSettings {
            temperature, tool, ..
        } = model_settings;
        let agent = model_client
            .agent(model.as_str())
            .preamble(&*super::prompt::PromptSection::Identity.build(&ctx).await?)
            .tools({
                if *tool {
                    crate::tools::FunctionTool::required_tools(Arc::clone(&ctx))?
                } else {
                    vec![]
                }
            })
            .temperature(**temperature)
            .default_max_turns(256)
            .build();
        Ok(agent)
    }
}
impl<C, P> super::Agent for LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<C> + 'static + Send + Sync,
{
    fn run(self: Box<Self>) -> crate::Result<JoinHandle<()>> {
        let handle = tokio::spawn(async move {
            let mut agent = *self;
            loop {
                tokio::select! {
                    message = agent.msg_receiver.recv() => {
                        if let Some(message) = message {
                            if let Some(message) = agent.user_message_filter(message) {
                                   agent.handle_message(message).await;
                            }
                        } else {
                            break;
                        }
                    },
                    ctl_signal = agent.ctl_signal_receiver.recv() => {
                        if let Some(signal) = ctl_signal {
                            agent.handle_ctl_signal(signal).await;
                        }
                    },
                    _ = tokio::signal::ctrl_c() => {
                        break;
                    }
                }
            }
            log::info!("agent[{}] run exited", agent.name);
        });
        Ok(handle)
    }

    fn msg_sender(&self) -> AgentMessageSender {
        self.msg_sender.clone()
    }
}

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<C> + 'static + Send + Sync,
{
    async fn handle_message(&mut self, agent_message: AgentMessage) {
        let (ref session_id, message) = match agent_message {
            AgentMessage::Private {
                session_id,
                message,
            } => (Some(session_id), message),
            AgentMessage::Group { message } => (None, message),
        };
        let _ = self
            .ctx
            .channel_message_sender
            .send(Self::create_channel_message(session_id, AgentSignal::Start))
            .await;
        let agent = self.agent.read().await;
        let mut stream = agent.stream_chat(message, self.history.clone()).await;
        while let Some(result) = stream.next().await {
            let agent_signal = match result {
                Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                    StreamedAssistantContent::ReasoningDelta { reasoning, .. } => {
                        Some(AgentSignal::ReasoningStream(
                            rig::completion::message::Reasoning::new(&reasoning),
                        ))
                    }
                    StreamedAssistantContent::Text(text) => {
                        Some(AgentSignal::MessageStream(Message::assistant(text.text())))
                    }
                    StreamedAssistantContent::ToolCall { tool_call, .. } => {
                        Some(AgentSignal::ToolCall(tool_call))
                    }
                    _ => None,
                },
                Ok(MultiTurnStreamItem::StreamUserItem(_)) => None,
                Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                    let usage = final_resp.usage();
                    let history = final_resp.history().expect("unexpected empty history!!!");
                    self.history = history.to_vec();
                    self.usage = usage;
                    Some(AgentSignal::Final(usage))
                }
                Ok(_) => None,
                Err(err) => Some(AgentSignal::Error(err.to_string())),
            };
            if let Some(agent_signal) = agent_signal {
                let _ = self
                    .ctx
                    .channel_message_sender
                    .send(Self::create_channel_message(session_id, agent_signal))
                    .await;
            }
        }
    }

    async fn handle_ctl_signal(&mut self, signal: AgentCtlSignal) {
        match signal {
            AgentCtlSignal::Reload { id, reason } => {
                warn!("Received reload signal[{}] with reason: {}", id, reason);
                let mut agent = self.agent.write().await;
                match Self::create_agent(
                    &self.model_provider,
                    &self.model,
                    &self.model_settings,
                    Arc::clone(&self.ctx),
                )
                .await
                {
                    Ok(update_agent) => {
                        *agent = update_agent;
                        warn!("Reload agent with signal[{}] success", id);
                        let _ = self
                            .ctx
                            .msg_sender
                            .send(AgentMessage::Group {
                                message: Message::user(
                                    "You have been reloaded, continue your conversation.",
                                ),
                            })
                            .await;
                    }
                    Err(err) => {
                        error!("Failed to reload agent: {}", err)
                    }
                }
            }
        }
    }
    fn user_message_filter(&self, agent_message: AgentMessage) -> Option<AgentMessage> {
        match agent_message {
            AgentMessage::Private {
                session_id,
                message,
            } => {
                if let Some(message) = self.user_message_filter_actual(message) {
                    return Some(AgentMessage::Private {
                        session_id,
                        message,
                    });
                }
            }
            AgentMessage::Group { message } => {
                if let Some(message) = self.user_message_filter_actual(message) {
                    return Some(AgentMessage::Group { message });
                }
            }
        }
        None
    }

    fn user_message_filter_actual(&self, message: Message) -> Option<Message> {
        if let Message::User { content, .. } = message {
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
                1 => Some(Message::User {
                    content: OneOrMany::one(vec.remove(0)),
                }),
                2.. => OneOrMany::many(vec)
                    .ok()
                    .map(|content| Message::User { content }),
            }
        } else {
            Some(message)
        }
    }

    fn create_channel_message(
        session_id: &Option<SessionId>,
        signal: AgentSignal,
    ) -> ChannelMessage {
        if let Some(session_id) = session_id {
            ChannelMessage::Private {
                session_id: session_id.clone(),
                signal,
            }
        } else {
            ChannelMessage::Group { signal }
        }
    }
}
