use crate::agent::{
    AgentContext, AgentId, AgentRequest, AgentResponse, AgentSettings, HistoryCompactResult,
    HistoryCompactVal, HistoryManager, LlmAgentSupplier, Workspace,
};
use crate::channels::{ChannelMessage, SessionId};
use crate::config::Config;
use crate::model_provider::{ModelName, ModelProvider, ModelSettings, ReasoningEffort};
use anyhow::anyhow;
use async_trait::async_trait;
use itertools::Itertools;
use log::{info, warn};
use rig::OneOrMany;
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::completion::{AssistantContent, Message, Usage};
use rig::message::UserContent;
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use serde_json::json;
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
    agent_settings: AgentSettings,
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
        Ok(Self {
            model_settings: model_provider
                .model_settings(&model_name)
                .map(|it| it.clone())
                .ok_or(anyhow!(anyhow!(
                    "model settings not found for {}",
                    agent_id
                )))?,
            agent_settings: ctx
                .config
                .agent_settings(&agent_id)
                .map(|it| it.clone())
                .unwrap_or_default(),
            model_name,
            model_provider,
            id: agent_id,
            ctx,
        })
    }

    pub async fn fork<ID: Into<AgentId>>(&self, agent_id: ID) -> crate::Result<Self> {
        Ok(Self {
            id: agent_id.into(),
            model_settings: self.model_settings.clone(),
            agent_settings: self.agent_settings.clone(),
            model_name: self.model_name.clone(),
            model_provider: self.model_provider.clone(),
            ctx: self.ctx.clone(),
        })
    }

    async fn create_agent(
        &self,
        reasoning_effort: ReasoningEffort,
    ) -> crate::Result<Agent<C::CompletionModel>>
    where
        P: ModelProvider<Client = C>,
    {
        let model_client = &self.model_provider.completion_client()?;
        let reasoning_effort = self
            .model_settings
            .reasoning_effort_mapping
            .from(reasoning_effort);
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
            .tools(crate::tools::FunctionTool::required_tools(Arc::clone(&self.ctx)).await?)
            .temperature(self.agent_settings.temperature)
            .default_max_turns(self.agent_settings.max_turns)
            .max_tokens(
                self.agent_settings
                    .max_tokens
                    .unwrap_or(self.model_settings.max_tokens),
            )
            .additional_params(json!( {
                "reasoning": {
                    "effort": reasoning_effort,
                }
            }))
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
        self.run_actual(request, channel_message_sender.clone())
            .await?;
        Ok(())
    }

    async fn session_compact(
        &self,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> HistoryCompactResult {
        let Some(history_manager) = self.ctx.history_manager.as_ref() else {
            return HistoryCompactResult::Ignore("history_manager not found!!!".into());
        };
        let (history, usage) = {
            let mgr = history_manager.read().await;
            match mgr.load(session_id, &self.id).await {
                Ok(result) => result,
                Err(err) => {
                    return HistoryCompactResult::Err(format!(
                        "history compact failed, err: {err}"
                    ));
                }
            }
        };
        let result = self
            .history_compact(&history_manager, session_id, compact_ratio, &history, usage)
            .await;
        result
    }
}

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    async fn run_actual(
        &self,
        request: AgentRequest,
        channel_message_sender: Sender<ChannelMessage>,
    ) -> crate::Result<()> {
        let Some(ref request @ AgentRequest { ref session_id, .. }) = self.request_filter(request)
        else {
            return Ok(());
        };
        let _ = channel_message_sender
            .send(ChannelMessage {
                session_id: session_id.clone(),
                message: AgentResponse::Start,
            })
            .await;
        let (history, _) = if let Some(mgr) = &self.ctx.history_manager {
            mgr.read()
                .await
                .load(session_id, &self.id)
                .await
                .unwrap_or_default()
        } else {
            (vec![], Default::default())
        };
        let agent = self
            .create_agent(self.agent_settings.reasoning_effort)
            .await?;
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
                        self.handle_history(
                            channel_message_sender.clone(),
                            mgr,
                            &request.session_id,
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
        Ok(())
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
                    id: Default::default(),
                    session_id: request.session_id,
                    message: Message::User {
                        content: OneOrMany::one(vec.remove(0)),
                    },
                }),
                2.. => OneOrMany::many(vec).ok().map(|content| AgentRequest {
                    id: Default::default(),
                    session_id: request.session_id,
                    message: Message::User { content },
                }),
            }
        } else {
            Some(request)
        }
    }

    async fn handle_history(
        &self,
        channel_message_sender: Sender<ChannelMessage>,
        history_manager: &Arc<RwLock<dyn HistoryManager>>,
        session_id: &SessionId,
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

        let max_tokens = self
            .agent_settings
            .max_tokens
            .unwrap_or(self.model_settings.max_tokens);
        if usage.total_tokens
            >= ((max_tokens as f32 * self.agent_settings.compact_threshold) as u64)
        {
            let _ = channel_message_sender
                .send(ChannelMessage {
                    session_id: session_id.clone(),
                    message: AgentResponse::Notify("Trigger history compact...".into()),
                })
                .await;

            let result = self
                .history_compact(
                    &history_manager,
                    session_id,
                    self.agent_settings.compact_threshold,
                    &history,
                    *usage,
                )
                .await;
            match &result {
                HistoryCompactResult::Ok(val) => {
                    info!("Compact session{session_id} history ok, {val}");
                }
                HistoryCompactResult::Ignore(msg) => {
                    info!("Compact session{session_id} ignore with {msg}, no history to compact");
                }
                HistoryCompactResult::Err(err) => {
                    warn!("Compact session{session_id} failed, err: {err}");
                }
            }
            let _ = channel_message_sender
                .send(ChannelMessage {
                    session_id: session_id.clone(),
                    message: AgentResponse::HistoryCompact(result),
                })
                .await;
        }
    }

    async fn history_compact(
        &self,
        history_manager: &Arc<RwLock<dyn HistoryManager>>,
        session_id: &SessionId,
        compact_ratio: f32,
        original_history: &[Message],
        original_usage: Usage,
    ) -> HistoryCompactResult {
        let ((head, _), (tail, tail_tokens)) = {
            let len = original_history.len();
            let ratio = 0.2f32.max(compact_ratio.min(1.));
            let size = (len as f32 * ratio) as usize;
            let (head, tail) = (&original_history[0..size], &original_history[size..]);
            if head.is_empty() {
                return HistoryCompactResult::Ignore(format!(
                    "the length of original history is {len}, compact-ratio: {ratio}, no history need to be compact..."
                ));
            }
            let head_tokens = (original_usage.total_tokens as f32 * ratio) as u64;
            let tail_tokens = original_usage.total_tokens - head_tokens;
            ((head.to_vec(), head_tokens), (tail.to_vec(), tail_tokens))
        };
        let agent = match self.create_agent(ReasoningEffort::Minimal).await {
            Ok(agent) => agent,
            Err(err) => return HistoryCompactResult::Err(format!("创建agent失败, err: {err}")),
        };
        let mut stream = agent.stream_chat(
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
                    let compacted = match final_resp.history().iter().flat_map(|&it| it).last() {
                        Some(it) => it,
                        None => {
                            return HistoryCompactResult::Err(
                                "unexpected empty compact result!!!".to_string(),
                            );
                        }
                    };
                    let compacted_usage = {
                        let compacted_usage = Usage {
                            total_tokens: usage.output_tokens + tail_tokens,
                            ..Default::default()
                        };
                        {
                            let mut history_manager = history_manager.write().await;
                            let (history_backup_path, backup_timestamp) = match history_manager
                                .backup(session_id, &self.id)
                                .await
                                .map_err(|err| anyhow!(err))
                            {
                                Ok(it) => it,
                                Err(err) => return HistoryCompactResult::Err(err.to_string()),
                            };
                            let Message::Assistant { id, content } = compacted else {
                                return HistoryCompactResult::Err(
                                    "unexpected non-assistant message in compacted history"
                                        .to_string(),
                                );
                            };
                            let compacted = Message::Assistant {
                                id: id.clone(),
                                content: {
                                    let mut content = content.clone();
                                    content.push(AssistantContent::text(format!(
                                        r#"
## Raw Data Backup Information
- Backup File Path: {}
- Processing Time: {}
- Status: Backup completed successfully

                                    "#,
                                        history_backup_path.display(),
                                        backup_timestamp,
                                    )));
                                    content
                                },
                            };
                            let _ = history_manager
                                .store(
                                    session_id,
                                    &self.id,
                                    &compacted_usage,
                                    &[&[compacted], tail.as_slice()].concat(),
                                )
                                .await;
                        }
                        compacted_usage
                    };
                    return HistoryCompactResult::Ok(HistoryCompactVal::new(
                        original_usage,
                        compacted_usage,
                    ));
                }
                Ok(_) => continue,
                Err(err) => {
                    return HistoryCompactResult::Err(format!(
                        "history compact failed , err: {err}"
                    ));
                }
            }
        }
        unreachable!("unexpected error, unreachable code")
    }
}
