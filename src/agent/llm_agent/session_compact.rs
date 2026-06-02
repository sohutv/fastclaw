use std::sync::Arc;
use crate::agent::llm_agent::LlmAgent;
use crate::agent::session_history::{HistoryMessage, StoreOption};
use crate::agent::{HistoryCompactResult, HistoryCompactVal, SessionCompactSupport};
use crate::channels::{ChannelMessage, SessionId};
use crate::model_provider::{ModelProvider, ReasoningEffort};
use anyhow::anyhow;
use async_trait::async_trait;
use itertools::Itertools;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::completion::{AssistantContent, Message, Usage};
use rig::streaming::StreamingChat;
use tokio::sync::mpsc::Sender;
use tokio_stream::StreamExt;

#[async_trait]
impl<C, P> SessionCompactSupport for LlmAgent<C, P>
where
    C: CompletionClient + 'static + Send + Sync,
    P: ModelProvider<Client = C> + 'static + Send + Sync,
{
    async fn session_compact(
        self: Arc<Self>,
        channel_message_sender: Sender<crate::Result<ChannelMessage>>,
        session_id: &SessionId,
        compact_ratio: f32,
    ) -> HistoryCompactResult {
        let (original_history, original_usage) =
            match self.ctx.history_manager.load(session_id, &self.id).await {
                Ok((messages, usage)) => (messages.into_iter().collect_vec(), usage),
                Err(err) => {
                    return HistoryCompactResult::Err(format!("{err}"));
                }
            };
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
        let agent = match Arc::clone(&self)
            .create_agent(
                session_id,
                ReasoningEffort::Minimal,
                None,
                channel_message_sender,
                |_| None,
            )
            .await
        {
            Ok(agent) => agent,
            Err(err) => return HistoryCompactResult::Err(format!("创建agent失败, err: {err}")),
        };
        let mut stream = agent.stream_chat(
            format!(
                r#"
**current session_id**: {}
Execute the 'slimming' maintenance of the conversation history immediately, generate a refined summary of the context.
{}
                            "#,
                session_id,
                include_str!("../../../resources/HISTORY_COMPACT.md")
            ),
            head.clone(),
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
                            let Message::Assistant { id, content } = compacted else {
                                return HistoryCompactResult::Err(
                                    "unexpected non-assistant message in compacted history"
                                        .to_string(),
                                );
                            };
                            let history_backup_path = match self
                                .ctx
                                .memory_manager
                                .create_index(
                                    session_id,
                                    &head
                                        .into_iter()
                                        .filter(|it| it.is_message())
                                        .map(|it| it.into())
                                        .collect_vec(),
                                )
                                .await
                                .map_err(|err| anyhow!(err))
                            {
                                Ok(it) => it,
                                Err(err) => return HistoryCompactResult::Err(err.to_string()),
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
- Status: Backup completed successfully"#,
                                        history_backup_path.display(),
                                        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
                                    )));
                                    content
                                },
                            };
                            let messages = {
                                let mut messages = vec![HistoryMessage::summary(compacted)];
                                let mut tail = tail.into_iter().collect_vec();
                                messages.append(&mut tail);
                                messages
                            };
                            let _ = self
                                .ctx
                                .history_manager
                                .store(
                                    session_id,
                                    &self.id,
                                    &compacted_usage,
                                    messages,
                                    StoreOption::Overwrite, // overwrite after summary
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
