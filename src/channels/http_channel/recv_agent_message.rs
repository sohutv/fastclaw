use crate::agent::{Agent, AgentId, AgentResponse, Notify};
use crate::channels::http_channel::type_::HttpReqMessage;
use crate::channels::http_channel::{Client, HttpChannel, Payload};
use crate::channels::http_channel::{HttpRespMessage, UserId};
use crate::channels::text_formater::{
    FormatedMessage, extract_message, extract_reasoning, format_history_compact, format_message,
    format_reasoning, format_tool_call,
};
use crate::channels::{
    AgentRespState, AgentRespType, ChannelContext, ChannelMessage, SessionId,
    create_robot_messages_for_agent,
};
use anyhow::anyhow;

impl HttpChannel {
    pub(super) async fn handle_agent_message_actual(
        &self,
        client: &Client,
        agent: &dyn Agent,
        inbound_message: Option<&HttpReqMessage>,
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
                format_tool_call(session_id, toolcall).map(|(text, rt)| (text.into(), rt)),
                curr_state,
            ),
            AgentResponse::ReasoningStream(reasoning) => {
                buff.extend(extract_reasoning(session_id, reasoning));
                (None, AgentRespState::Reasoning)
            }
            AgentResponse::MessageStream(message) => {
                let formated_message = if let AgentRespState::Reasoning = curr_state {
                    Some(format_reasoning(session_id, buff)).map(|(text, rt)| (text.into(), rt))
                } else {
                    None
                };
                buff.extend(extract_message(session_id, message));
                (formated_message, AgentRespState::Messaging)
            }
            AgentResponse::Final(usage) => (
                Some(format_message(
                    session_id,
                    agent.agent_settings().output_schema.is_some(),
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
                Some(format_history_compact(session_id, result))
                    .map(|(text, rt)| (text.into(), rt)),
                curr_state,
            ),
        };
        if let Some((text, resp_type)) = formated_message {
            if let Some(robot_message) = create_robot_messages_for_agent(
                agent,
                session_id,
                &self.ctx,
                resp_type,
                inbound_message,
                text,
                HttpChannel::create_resp_messages,
            )
            .await?
            {
                let _ = robot_message.send(client, session_id, agent_id).await;
            }
        }
        Ok(next_state)
    }
}

impl HttpChannel {
    fn create_resp_messages(
        _: &dyn Agent,
        session_id: &SessionId,
        _: &ChannelContext,
        input: Option<&HttpReqMessage>,
        content: FormatedMessage,
    ) -> crate::Result<HttpRespMessage> {
        let output = content.into();
        match &session_id {
            SessionId::Master { .. } | SessionId::Anonymous { .. } => Ok(HttpRespMessage {
                output,
                input: input.map(|it| it.clone()),
            }),
            SessionId::Group { .. } => Err(anyhow!(
                "send robot message to group is not supported by http"
            )),
        }
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

impl HttpRespMessage {
    async fn send(self, client: &Client, session_id: &SessionId, agent_id: &AgentId) {
        let user_id = UserId::from(session_id);
        if let Some(guard) = client.read().await.get(&user_id) {
            let mut user_transports = guard.write().await;
            if let Some((agent_id, agent_transports)) = user_transports.remove_entry(agent_id) {
                let mut updated = vec![];
                for transport in agent_transports {
                    let sender = &transport.sender;
                    if sender.is_closed() {
                        log::warn!(
                            "failed to send resp message, transport had been closed, user_id: {}, agent_id: {} ",
                            user_id,
                            agent_id
                        );
                    } else {
                        let _ = sender.send(self.clone()).await;
                        updated.push(transport)
                    }
                }
                if !updated.is_empty() {
                    user_transports.insert(agent_id, updated);
                }
            }
        }
    }
}
