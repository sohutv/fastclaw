use crate::agent::llm_agent::LlmAgent;
use crate::agent::{AgentRequest, AgentRequestContext, AgentResponse};
use crate::channels::ChannelMessage;
use crate::model_provider::ModelProvider;
use itertools::Itertools;
use log::warn;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::completion::Message;
use rig::message::UserContent;
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use std::sync::Arc;
use tokio_stream::StreamExt;

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    pub(super) async fn handle_request(
        self: Arc<Self>,
        AgentRequest {
            ref session_id,
            mut message,
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
        let message = match message {
            Message::System { .. } => message,
            Message::User {
                ref mut content, ..
            } => {
                content.push(UserContent::text(format!(
                    "- **Current DateTime**: {}",
                    chrono::Local::now().to_rfc3339()
                )));
                message
            }
            Message::Assistant { .. } => message,
        };
        let mut stream = agent.stream_chat(message, history).await;
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
