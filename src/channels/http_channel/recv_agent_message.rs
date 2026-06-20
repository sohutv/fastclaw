use crate::agent::{AgentResponse, Notify};
use crate::channels::http_channel::{HttpChannel, HttpClient, Payload};
use crate::channels::text_formater::{
    FormatedMessage, extract_message, extract_reasoning, format_history_compact, format_message,
    format_reasoning, format_tool_call,
};
use crate::channels::{AgentRespState, AgentRespType, ChannelMessage, create_outbound_msg};
use anyhow::anyhow;

impl HttpChannel {
    pub(super) async fn handle_agent_message_actual(
        &self,
        http_client: &HttpClient,
        ChannelMessage {
            session_id,
            agent_id,
            message,
        }: &ChannelMessage,
        curr_state: AgentRespState,
        buff: &mut Vec<String>,
    ) -> crate::Result<AgentRespState> {
        let (formated_message, next_state) = match message {
            AgentResponse::Start => {
                let AgentRespState::Wait = curr_state else {
                    return Err(anyhow!("AgentRespState must be Init when starting"));
                };
                buff.clear();
                (None, AgentRespState::Start)
            }
            AgentResponse::ToolCall(toolcall) => (
                format_tool_call(session_id, self.http_config, toolcall)
                    .map(|(text, rt)| (text.into(), rt)),
                curr_state,
            ),
            AgentResponse::ReasoningStream(reasoning) => {
                buff.extend(extract_reasoning(session_id, self.http_config, reasoning));
                (None, AgentRespState::Reasoning)
            }
            AgentResponse::MessageStream(message) => {
                let formated_message = if let AgentRespState::Reasoning = curr_state {
                    Some(format_reasoning(session_id, buff)).map(|(text, rt)| (text.into(), rt))
                } else {
                    None
                };
                buff.extend(extract_message(session_id, self.http_config, message));
                (formated_message, AgentRespState::Messaging)
            }
            AgentResponse::Final(usage) => (
                Some(format_message(
                    session_id,
                    self.http_config,
                    self.agent.agent_settings().output_schema.is_some(),
                    usage,
                    buff,
                )?),
                AgentRespState::Final,
            ),
            AgentResponse::Error(error) => (
                Some((
                    format!("Agent error: {}", error).into(),
                    AgentRespType::Error,
                )),
                AgentRespState::Final,
            ),
            AgentResponse::Notify(notify) => (
                Some((
                    match notify {
                        Notify::Text(text) => text.to_string(),
                        Notify::Markdown { content, .. } => format!("{content}",),
                    }
                    .into(),
                    AgentRespType::Notify,
                )),
                curr_state,
            ),
            AgentResponse::HistoryCompact(result) => (
                Some(format_history_compact(session_id, self.http_config, result))
                    .map(|(text, rt)| (text.into(), rt)),
                curr_state,
            ),
        };
        if let Some((text, resp_type)) = formated_message {
            if let Some(robot_message) = create_outbound_msg(
                http_client,
                &*self.agent,
                session_id,
                self.http_config,
                &self.context,
                resp_type,
                text,
                HttpChannel::create_resp_messages,
            )
            .await?
            {
                let _ = robot_message.send(http_client, session_id, agent_id).await;
            }
        }
        Ok(next_state)
    }
}

impl From<FormatedMessage> for Payload {
    fn from(value: FormatedMessage) -> Self {
        match value {
            FormatedMessage::Markdown(text) => text.into(),
            FormatedMessage::Json(json) => json.into(),
        }
    }
}
