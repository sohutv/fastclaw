use crate::agent::{Agent, AgentResponse, Notify};
use crate::channels::a2a_channel::A2AChannel;
use crate::channels::text_formater::*;
use crate::channels::{AgentRespState, AgentRespType};
use crate::channels::{ChannelContext, ChannelMessage, SessionId, create_outbound_msg};
use anyhow::anyhow;

impl A2AChannel {
    pub(crate) async fn handle_agent_message_actual(
        &self,
        client: &(),
        ChannelMessage {
            session_id,
            message,
            agent_id: message_from,
            ..
        }: &ChannelMessage,
        curr_state: AgentRespState,
        buff: &mut Vec<String>,
    ) -> crate::Result<(AgentRespState, Option<String>)> {
        let (formated_message, next_state) = match message {
            AgentResponse::Start => {
                let AgentRespState::Wait = curr_state else {
                    return Err(anyhow!("AgentRespState must be Init when starting"));
                };
                buff.clear();
                (None, AgentRespState::Start)
            }
            AgentResponse::ToolCall(toolcall) => (
                format_tool_call(session_id, &self.config, toolcall),
                curr_state,
            ),
            AgentResponse::ReasoningStream(reasoning) => {
                buff.extend(extract_reasoning(session_id, &self.config, reasoning));
                (None, AgentRespState::Reasoning)
            }
            AgentResponse::MessageStream(message) => {
                let formated_message = if let AgentRespState::Reasoning = curr_state {
                    Some(format_reasoning(session_id, buff))
                } else {
                    None
                };
                buff.extend(extract_message(session_id, &self.config, message));
                (formated_message, AgentRespState::Messaging)
            }
            AgentResponse::Final(usage) => (
                Some(
                    format_message(
                        session_id,
                        &self.config,
                        self.context
                            .agent_registry
                            .get(message_from)
                            .await?
                            .agent_settings()
                            .output_schema
                            .is_some(),
                        usage,
                        buff,
                    )
                    .map(|(msg, rt)| (msg.to_string(), rt))?,
                ),
                AgentRespState::Final,
            ),
            AgentResponse::Error(error) => (
                Some((format!("Agent error: {}", error), AgentRespType::Error)),
                AgentRespState::Final,
            ),
            AgentResponse::Notify(notify) => (
                Some((
                    match notify {
                        Notify::Text(text) => text.to_string(),
                        Notify::Markdown { content, .. } => format!("{content}",),
                    },
                    AgentRespType::Notify,
                )),
                curr_state,
            ),
            AgentResponse::HistoryCompact(result) => (
                Some(format_history_compact(session_id, &self.config, result)),
                curr_state,
            ),
        };
        if let Some((text, resp_type)) = formated_message {
            if let Some(robot_message) = create_outbound_msg(
                client,
                &*self.context.agent_registry.get(message_from).await?,
                session_id,
                &self.config,
                &self.context,
                resp_type,
                text,
                A2AChannel::create_robot_messages,
            )
            .await?
            {
                return Ok((next_state, Some(robot_message)));
            }
        }
        Ok((next_state, None))
    }
}

impl A2AChannel {
    fn create_robot_messages(
        _: &(),
        _: &dyn Agent,
        _: &SessionId,
        _: &ChannelContext,
        content: String,
    ) -> crate::Result<String> {
        Ok(content)
    }
}
