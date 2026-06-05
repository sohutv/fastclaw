use crate::agent::llm_agent::LlmAgent;
use crate::agent::{
    AgentRequest, AgentRequestContext, AgentRequestPkg, AgentResponse,
    TaskBackpressure,
};
use crate::channels::ChannelMessage;
use crate::model_provider::ModelProvider;
use itertools::Itertools;
use log::warn;
use rig::OneOrMany;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::completion::Message;
use rig::message::UserContent;
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio_stream::StreamExt;

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
                        create_time: _,
                    }) = rx.recv().await
                    {
                        let _ = Arc::clone(&self_).handle_request_actual(req, ctx).await;
                        if let Some(ack_sender) = ack_sender {
                            let _ = ack_sender.send(());
                        }
                    }
                });
            }
            TaskBackpressure::Latest => {
                let self_ = Arc::clone(&self);
                let (watch_tx, mut watch_rx) = tokio::sync::watch::channel(None);
                tokio::spawn(async move {
                    while let Some(AgentRequestPkg {
                        req,
                        ctx,
                        ack_sender,
                        create_time: _,
                    }) = rx.recv().await
                    {
                        let _ = watch_tx.send(Some((req, ctx)));
                        if let Some(ack_sender) = ack_sender {
                            let _ = ack_sender.send(());
                        }
                    }
                });
                tokio::spawn(async move {
                    while let Ok(_) = watch_rx.changed().await {
                        if let Some((req, ctx)) = {
                            let dst = watch_rx.borrow();
                            dst.deref().clone()
                        } {
                            let _ = Arc::clone(&self_).handle_request_actual(req, ctx).await;
                        }
                    }
                });
            }
        }
    }

    async fn handle_request_actual(
        self: Arc<Self>,
        AgentRequest {
            ref session_id,
            message,
            ..
        }: AgentRequest,
        AgentRequestContext {
            channel_message_sender,
            addi_system_prompt,
            tool_filter,
            with_history,
        }: AgentRequestContext,
    ) {
        let _ = channel_message_sender
            .send(Ok(ChannelMessage {
                session_id: session_id.clone(),
                agent_id: self.id.clone(),
                message: AgentResponse::Start,
            }))
            .await;
        let agent = match Arc::clone(&self)
            .create_agent(
                session_id,
                self.agent_settings.reasoning_effort,
                addi_system_prompt.as_deref(),
                channel_message_sender.clone(),
                tool_filter,
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
        let history: Vec<Message> = if with_history
            && let Some(chat_history_limit @ 1..) = self.agent_settings.chat_history_limit
        {
            let (history, _) = self
                .ctx
                .history_manager
                .load(session_id, &self.id)
                .await
                .unwrap_or_default();
            let history = history.into_iter().map(|it| it.into()).collect_vec();
            if chat_history_limit < history.len() {
                history
                    .into_iter()
                    .rev()
                    .take(chat_history_limit)
                    .rev()
                    .collect_vec()
            } else {
                history
            }
        } else {
            vec![]
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
        let mut stream = agent
            .stream_chat(merge_user_message(message), history)
            .await;
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
