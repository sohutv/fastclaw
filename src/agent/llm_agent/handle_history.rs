use crate::agent::llm_agent::LlmAgent;
use crate::agent::session_history::{HistoryMessage, StoreOption};
use crate::agent::{AgentResponse, HistoryCompactResult, SessionCompactSupport};
use crate::channels::{ChannelMessage, SessionId};
use crate::model_provider::ModelProvider;
use itertools::Itertools;
use log::{info, warn};
use rig::client::CompletionClient;
use rig::completion::{Message, Usage};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

impl<C, P> LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    pub(super) async fn handle_history(
        self: Arc<Self>,
        channel_message_sender: Sender<crate::Result<ChannelMessage>>,
        session_id: &SessionId,
        usage: &Usage,
        append_history: &[Message],
    ) {
        match self
            .ctx
            .history_manager
            .store(
                session_id,
                &self.id,
                &usage,
                append_history
                    .iter()
                    .map(|it| HistoryMessage::message(it.clone()))
                    .collect_vec(),
                StoreOption::Append,
            )
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
        let max_tokens = self
            .agent_settings
            .max_tokens
            .unwrap_or(self.model_settings.max_tokens);
        if usage.total_tokens
            >= ((max_tokens as f32 * self.agent_settings.compact_threshold) as u64)
        {
            let _ = channel_message_sender
                .send(Ok(ChannelMessage {
                    session_id: session_id.clone(),
                    message: AgentResponse::Notify("Trigger history compact...".into()),
                }))
                .await;

            let result = Arc::clone(&self)
                .session_compact(
                    channel_message_sender.clone(),
                    session_id,
                    self.agent_settings.compact_threshold,
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
                .send(Ok(ChannelMessage {
                    session_id: session_id.clone(),
                    message: AgentResponse::HistoryCompact(result),
                }))
                .await;
        }
    }
}
