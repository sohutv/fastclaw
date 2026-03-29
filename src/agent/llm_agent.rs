use crate::agent::{
    AgentContext, AgentId, AgentRequest, AgentResponse, HistoryCompactResult, HistoryCompactVal,
    HistoryManager, LlmAgentSupplier, Notify, Workspace,
};
use crate::channels::{ChannelMessage, SessionId};
use crate::config::Config;
use crate::model_provider::{ModelContext, ModelName, ModelProvider, ModelSettings};
use async_trait::async_trait;
use itertools::Itertools;
use log::{info, warn};
use rig::OneOrMany;
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::completion::{Message, Usage};
use rig::message::UserContent;
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct LlmAgent<C, P>
where
    C: CompletionClient,
    P: ModelProvider<Client = C>,
{
    id: AgentId,
    ctx: Arc<AgentContext>,
    model_provider: P,
    model_name: ModelName,
    model_settings: ModelSettings,
}

#[async_trait]
impl<C, P> LlmAgentSupplier for P
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    type A = LlmAgent<C, P>;

    async fn create_agent<ID: Into<AgentId> + Send>(
        &self,
        agent_id: ID,
        config: &'static Config,
        model: ModelName,
        history_manager: Option<Arc<RwLock<dyn HistoryManager>>>,
        workspace: &'static Workspace,
    ) -> crate::Result<Self::A> {
        Ok(LlmAgent::new(
            agent_id.into(),
            config,
            self.clone(),
            model,
            history_manager,
            workspace,
        )
        .await?)
    }
}

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient,
    P: ModelProvider<Client = C>,
{
    async fn new(
        agent_id: AgentId,
        config: &'static Config,
        model_provider: P,
        model_name: ModelName,
        history_manager: Option<Arc<RwLock<dyn HistoryManager>>>,
        workspace: &'static Workspace,
    ) -> crate::Result<Self> {
        let ctx = Arc::new(AgentContext {
            config,
            workspace,
            history_manager,
        });
        let model_settings = *model_provider
            .model_settings(&model_name)
            .expect("model settings not found");
        Ok(Self {
            id: agent_id.into(),
            ctx,
            model_provider,
            model_name,
            model_settings,
        })
    }

    async fn create_agent(&self) -> crate::Result<Agent<C::CompletionModel>>
    where
        P: ModelProvider<Client = C>,
    {
        let model_client = &self.model_provider.completion_client()?;
        let ModelSettings { temperature, .. } = &self.model_settings;
        let agent = model_client
            .agent(&*self.model_name)
            .preamble(
                &*super::prompt::PromptSection::Identity
                    .build(&self.ctx)
                    .await?,
            )
            .append_preamble(&format!(
                r#"
# MetaData
- **Your AgentId**: {}
            "#,
                &self.id
            ))
            .tools(crate::tools::FunctionTool::required_tools(Arc::clone(
                &self.ctx,
            ))?)
            .temperature(**temperature)
            .default_max_turns(256)
            .build();
        Ok(agent)
    }
}

#[async_trait]
impl<C, P> super::Agent for LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    async fn run(
        &self,
        request: AgentRequest,
        channel_message_sender: Sender<ChannelMessage>,
    ) -> crate::Result<()> {
        self.handle_message(request, channel_message_sender.clone())
            .await;
        Ok(())
    }

    async fn session_compact(
        &self,
        channel_message_sender: Sender<ChannelMessage>,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> HistoryCompactResult {
        let result = self
            .session_history_compact(session_id, compact_ratio)
            .await
            .unwrap_or_else(|result| result);
        let _ = channel_message_sender
            .send(ChannelMessage {
                session_id: session_id.clone(),
                message: AgentResponse::HistoryCompact(result.clone()),
            })
            .await;
        result
    }
}

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
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
        let history = if let Some(mgr) = &self.ctx.history_manager {
            mgr.read()
                .await
                .load(
                    session_id,
                    &self.id,
                    self.model_settings.context.window_size,
                )
                .await
                .unwrap_or_default()
        } else {
            vec![]
        };

        let agent = self.create_agent().await.unwrap();
        let mut stream = agent.stream_chat(request.clone(), history).await;
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
                    if let Some(mgr) = &self.ctx.history_manager {
                        self.handle_session_history(
                            channel_message_sender.clone(),
                            mgr,
                            &request.session_id,
                            &self.model_settings.context,
                            &usage,
                            history,
                        )
                        .await;
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

    async fn handle_session_history(
        &self,
        channel_message_sender: Sender<ChannelMessage>,
        history_manager: &Arc<RwLock<dyn HistoryManager>>,
        session_id: &SessionId,
        ModelContext {
            max_tokens,
            compact_threshold,
            ..
        }: &ModelContext,
        usage: &Usage,
        history: &[Message],
    ) {
        {
            match history_manager
                .write()
                .await
                .store(session_id, &self.id, &usage, history)
                .await
            {
                Ok(_) => {}
                Err(err) => {
                    warn!(
                        "Store history failed, session_id: {}, agent: {}, err: {}",
                        session_id, self.id, err
                    );
                }
            }
        }
        if usage.total_tokens >= ((*max_tokens as f32 * compact_threshold) as u64) {
            let _ = channel_message_sender
                .send(ChannelMessage {
                    session_id: session_id.clone(),
                    message: AgentResponse::Notify("Trigger history compact...".into()),
                })
                .await;
            match selfa
                .session_history_compact(session_id, *compact_threshold)
                .await
            {
                Ok(result) => {
                    match &result {
                        HistoryCompactResult::Ok(val) => {
                            info!("Compact session{session_id} history ok, {val}");
                        }
                        HistoryCompactResult::Ignore(msg) => {
                            info!(
                                "Compact session{session_id} ignore with {msg}, no history to compact"
                            );
                        }
                        _ => unreachable!(),
                    }
                    let _ = channel_message_sender
                        .send(ChannelMessage {
                            session_id: session_id.clone(),
                            message: AgentResponse::HistoryCompact(result),
                        })
                        .await;
                }
                Err(result) => {
                    let HistoryCompactResult::Err(err) = &result else {
                        unreachable!()
                    };
                    info!("Compact session{session_id} failed, err: {err}");
                    let _ = channel_message_sender
                        .send(ChannelMessage {
                            session_id: session_id.clone(),
                            message: AgentResponse::HistoryCompact(result),
                        })
                        .await;
                }
            }
        }
    }

    async fn session_history_compact(
        &self,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> crate::Result<HistoryCompactResult, HistoryCompactResult> {
        let Some(history_manager) = self.ctx.history_manager.as_ref() else {
            return Ok(HistoryCompactResult::Ignore(
                "history_manager not found!!!".into(),
            ));
        };
        let (original_history, original_usage) = {
            let mgr = history_manager.read().await;
            mgr.load_with_offset(session_id, &self.id, None, None)
                .await
                .map_err(|err| {
                    HistoryCompactResult::Err(format!("history compact failed, err: {err}"))
                })?
        };
        let ((head, _), (tail, tail_tokens)) = {
            let len = original_history.len();
            let ratio = 0.2f32.max(compact_ratio.min(1.));
            let size = (len as f32 * ratio) as usize;
            let (head, tail) = (&original_history[0..size], &original_history[size..]);
            if head.is_empty() {
                return Ok(HistoryCompactResult::Ignore(format!(
                    "the length of original history is {len}, compact-ratio: {ratio}, no history need to be compact..."
                )));
            }
            let head_tokens = (original_usage.total_tokens as f32 * ratio) as u64;
            let tail_tokens = original_usage.total_tokens - head_tokens;
            ((head.to_vec(), head_tokens), (tail.to_vec(), tail_tokens))
        };
        let mut stream = self
            .create_agent()
            .await
            .map_err(|err| HistoryCompactResult::Err(format!("创建agent失败, err: {err}")))?
            .stream_chat(
                format!(
                    r#"
**current session_id**: {}
Execute the 'slimming' maintenance of the conversation history immediately: back up the history and generate a refined summary of the context.
{}
                            "#,
                    session_id,
                    include_str!("./prompt/history_compact_prompt.md")
                ),
                head,
            )
            .await;
        while let Some(item) = stream.next().await {
            match item {
                Ok(MultiTurnStreamItem::FinalResponse(final_resp)) => {
                    let usage = final_resp.usage();
                    let compacted = final_resp
                        .history()
                        .filter(|&it| it.len() > 0)
                        .map(|it| &it[it.len() - 1..])
                        .expect("unexpected empty history!!!");
                    let compacted_usage = {
                        let compacted_usage = Usage {
                            total_tokens: usage.total_tokens + tail_tokens,
                            ..Default::default()
                        };
                        let _ = history_manager
                            .write()
                            .await
                            .store(
                                session_id,
                                &self.id,
                                &compacted_usage,
                                &[compacted, tail.as_slice()].concat(),
                            )
                            .await;
                        compacted_usage
                    };
                    return Ok(HistoryCompactResult::Ok(HistoryCompactVal::new(
                        original_usage,
                        compacted_usage,
                    )));
                }
                Ok(_) => continue,
                Err(err) => {
                    return Err(HistoryCompactResult::Err(format!(
                        "history compact failed , err: {err}"
                    )));
                }
            }
        }
        unreachable!("unexpected error, unreachable code")
    }
}
