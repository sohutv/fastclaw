use crate::agent::llm_agent::LlmAgent;
use crate::agent::{
    AgentRequest, AgentRequestContext, AgentRequestPkg, AgentResponse, TaskBackpressure,
};
use crate::channels::ChannelMessage;
use crate::model_provider::ModelProvider;
use futures_util::StreamExt;
use itertools::Itertools;
use log::{info, warn};
use rig::OneOrMany;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::completion::Message;
use rig::message::UserContent;
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::Receiver;

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    pub(super) async fn handle_request(self: Arc<Self>, mut rx: Receiver<AgentRequestPkg>) {
        match self.agent_settings.task_backpressure {
            TaskBackpressure::Pending => {
                let self_ = Arc::clone(&self);
                tokio::spawn(async move {
                    while let Some(AgentRequestPkg {
                        req,
                        ctx,
                        ack_sender,
                        ..
                    }) = rx.recv().await
                    {
                        let id = req.id.clone();
                        let start = chrono::Local::now();
                        let _ = Arc::clone(&self_).handle_request_actual(req, ctx).await;
                        if let Some(ack_sender) = ack_sender {
                            let _ = ack_sender.send(());
                        }
                        info!(
                            "[Pending] handle_request ack AgentRequestPkg {} ok /elapsed: {:?}ms",
                            id,
                            (chrono::Local::now() - start).num_milliseconds()
                        );
                    }
                });
            }
            TaskBackpressure::Latest => {
                let self_ = Arc::clone(&self);
                let (watch_tx, mut watch_rx) = tokio::sync::watch::channel(Default::default());
                tokio::spawn(async move {
                    while let Some(AgentRequestPkg {
                        req,
                        ctx,
                        ack_sender,
                        create_time: _,
                    }) = rx.recv().await
                    {
                        let now = chrono::Local::now();
                        let before =
                            watch_tx.send_replace(Arc::new(Mutex::new(Some((req, ctx, now)))));
                        tokio::spawn(async move {
                            let mut guard = before.lock().await;
                            if let Some(dst) = guard.take() {
                                drop(dst);
                            }
                        });
                        if let Some(ack_sender) = ack_sender {
                            let _ = ack_sender.send(());
                        }
                    }
                });
                tokio::spawn(async move {
                    while let Ok(_) = watch_rx.changed().await {
                        let dst = {
                            let borrow_val = watch_rx.borrow().clone();
                            borrow_val.lock().await.take()
                        };
                        if let Some((req, ctx, received_time)) = dst {
                            let id = req.id.clone();
                            let start = chrono::Local::now();
                            let _ = Arc::clone(&self_).handle_request_actual(req, ctx).await;
                            info!(
                                "[Latest] handle_request changed AgentRequestPkg {} ok, /received_at: {}, gap: {}ms, elapsed: {}ms",
                                id,
                                received_time,
                                (start - received_time).num_milliseconds(),
                                (chrono::Local::now() - start).num_milliseconds()
                            );
                        }
                    }
                });
            }
        }
    }

    async fn handle_request_actual(
        self: Arc<Self>,
        agent_request: AgentRequest,
        AgentRequestContext {
            channel_message_sender,
            tool_filter,
            with_history,
        }: AgentRequestContext,
    ) {
        let _ = channel_message_sender
            .send(Ok(ChannelMessage {
                session_id: agent_request.session_id.clone(),
                agent_id: self.id.clone(),
                message: AgentResponse::Start,
            }))
            .await;
        let agent = match self
            .create_agent(
                agent_request.session_id.clone(),
                agent_request.addi_system_prompt.as_deref(),
                channel_message_sender.clone(),
                tool_filter,
                Some(&agent_request),
            )
            .await
        {
            Ok(agent) => agent,
            Err(err) => {
                warn!("create_agent failed, err: {err}");
                let _ = channel_message_sender.send(Err(err)).await;
                return;
            }
        };
        let AgentRequest {
            ref session_id,
            message,
            ..
        } = agent_request;

        let history: Vec<Message> = match (
            with_history,
            self.agent_settings.chat_history_limit.unwrap_or(usize::MAX),
        ) {
            (true, limit @ 1..) => {
                let (history, _) = self
                    .ctx
                    .history_manager
                    .load(session_id, &self.id)
                    .await
                    .unwrap_or_default();
                let history = history.into_iter().map(|it| it.into()).collect_vec();
                if limit < history.len() {
                    let mut array = Vec::with_capacity(history.len());
                    let mut cnt = 0;
                    for message in history.into_iter().rev() {
                        if let Message::User { .. } = &message {
                            array.push(message);
                            cnt += 1;
                        } else {
                            array.push(message);
                        }
                        if cnt >= limit {
                            break;
                        }
                    }
                    array
                } else {
                    history
                }
            }
            _ => vec![],
        };
        #[inline(always)]
        fn merge_user_message(messages: Vec<OneOrMany<UserContent>>) -> Message {
            match OneOrMany::many(
                messages
                    .into_iter()
                    .flatten()
                    .chain(vec![UserContent::text(format!(
                        "- **Current DateTime**: {}",
                        chrono::Local::now().to_rfc3339()
                    ))])
                    .collect_vec(),
            ) {
                Ok(content) => Message::User { content },
                Err(_) => {
                    unreachable!("unreachable empty list error");
                }
            }
        }
        let input_message = merge_user_message(message);
        let mut stream = agent.stream_chat(input_message, history).await;
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
                    let append_history = final_resp.history().expect("unexpected empty history!!!");
                    if with_history {
                        Arc::clone(&self)
                            .append_history(
                                channel_message_sender.clone(),
                                session_id,
                                &usage,
                                append_history,
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
                    .send(Ok(ChannelMessage {
                        session_id: session_id.clone(),
                        agent_id: self.id.clone(),
                        message,
                    }))
                    .await;
            }
        }
    }
}
