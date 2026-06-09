use crate::agent::{Agent, AgentId, AgentResponse, HistoryCompactResult, Notify};
use crate::channels::http_channel::type_::HttpReqMessage;
use crate::channels::http_channel::{Client, HttpChannel, Payload};
use crate::channels::http_channel::{HttpRespMessage, UserId};
use crate::channels::{
    AgentRespState, AgentRespType, ChannelContext, ChannelMessage, SessionId,
    create_robot_messages_for_agent,
};
use anyhow::anyhow;
use log::info;
use rig::completion::{AssistantContent, Message};
use rig::message::{ReasoningContent, ToolCall, ToolFunction};

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
        match message {
            AgentResponse::Start => {
                if let AgentRespState::Wait = curr_state {
                    buff.clear();
                    Ok(AgentRespState::Start)
                } else {
                    Err(anyhow!("AgentRespState must be Init when starting"))
                }
            }
            AgentResponse::ToolCall(ToolCall {
                function: ToolFunction { name, arguments },
                ..
            }) => {
                if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                    agent,
                    session_id,
                    &self.ctx,
                    AgentRespType::ToolCall,
                    inbound_message,
                    {
                        let text = format!(
                            r#"### 工具调用: {name}...
```
{}
```json
"#,
                            serde_json::to_string_pretty(arguments).unwrap_or_else(|err| format!(
                                "Error serializing arguments: {}",
                                err
                            ))
                        );
                        info!(
                            "[{:?}] agent resp {text}",
                            inbound_message.map(|it| &it.message_id)
                        );
                        text
                    },
                    HttpChannel::create_resp_messages,
                )
                .await
                {
                    let _ = robot_message.send(client, session_id, agent_id).await;
                }
                Ok(curr_state)
            }
            AgentResponse::ReasoningStream(reasoning) => {
                match curr_state {
                    AgentRespState::Start => if session_id.settings().show_reasoning {},
                    _ => {}
                }
                for content in reasoning.content.iter() {
                    if let ReasoningContent::Text { text, .. } = content {
                        if !text.is_empty() {
                            buff.push(text.clone());
                        }
                    }
                }
                Ok(AgentRespState::Reasoning)
            }
            AgentResponse::MessageStream(message) => {
                match curr_state {
                    AgentRespState::Start => {}
                    AgentRespState::Reasoning => {
                        if session_id.settings().show_reasoning {
                            let content = {
                                let content = buff.join("");
                                buff.clear();
                                {
                                    let text = format!(
                                        r#"### 我的想法..
{content}
"#
                                    );
                                    info!(
                                        "[{:?}] agent resp {text}",
                                        inbound_message.map(|it| &it.message_id)
                                    );
                                    text
                                }
                            };
                            if let Some(robot_message) = create_robot_messages_for_agent(
                                agent,
                                session_id,
                                &self.ctx,
                                AgentRespType::Reasoning,
                                inbound_message,
                                content,
                                HttpChannel::create_resp_messages,
                            )
                            .await?
                            {
                                let _ = robot_message.send(client, session_id, agent_id).await;
                            }
                        }
                    }
                    _ => {}
                }
                match message {
                    Message::Assistant { content, .. } => {
                        for content in content.iter() {
                            match content {
                                AssistantContent::Text(text) => {
                                    let text_str = text.to_string();
                                    if !text_str.is_empty() {
                                        buff.push(text_str);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                Ok(AgentRespState::Messaging)
            }
            AgentResponse::Final(usage) => {
                let content = {
                    let text = buff.join("");
                    info!(
                        "[{:?}] agent resp {text}",
                        inbound_message.map(|it| &it.message_id)
                    );
                    let token_usage = format!(
                        "*<<Tokens:{}↑{}↓{}>>*",
                        usage.total_tokens, usage.input_tokens, usage.output_tokens
                    );
                    buff.clear();
                    if session_id.settings().show_token_usage {
                        format!(
                            r#"
{text}

{token_usage}
"#,
                        )
                    } else {
                        format!(
                            r#"
{text}
"#
                        )
                    }
                };
                if let Some(robot_message) = create_robot_messages_for_agent(
                    agent,
                    session_id,
                    &self.ctx,
                    AgentRespType::Content,
                    inbound_message,
                    content,
                    HttpChannel::create_resp_messages,
                )
                .await?
                {
                    let _ = robot_message.send(client, session_id, agent_id).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Error(error) => {
                if let Some(robot_message) = create_robot_messages_for_agent(
                    agent,
                    session_id,
                    &self.ctx,
                    AgentRespType::Error,
                    inbound_message,
                    format!("Agent error: {}", error),
                    HttpChannel::create_resp_messages,
                )
                .await?
                {
                    let _ = robot_message.send(client, session_id, agent_id).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Notify(notify) => {
                match notify {
                    Notify::Text(text) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            agent,
                            session_id,
                            &self.ctx,
                            AgentRespType::Notify,
                            inbound_message,
                            text.clone(),
                            HttpChannel::create_resp_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(client, session_id, agent_id).await;
                        }
                    }
                    Notify::Markdown { content, .. } => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            agent,
                            session_id,
                            &self.ctx,
                            AgentRespType::Notify,
                            inbound_message,
                            format!("{content}",),
                            HttpChannel::create_resp_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(client, session_id, agent_id).await;
                        }
                    }
                }
                Ok(curr_state)
            }
            AgentResponse::HistoryCompact(result) => {
                match result {
                    HistoryCompactResult::Ok(val) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            agent,
                            session_id,
                            &self.ctx,
                            AgentRespType::HistoryCompactOk,
                            inbound_message,
                            format!(
                                r#"### 压缩上下文完成
- 压缩前 **{}** Tokens
- 压缩后 **{}** Tokens
- 压缩率 **{:.2}%**
"#,
                                val.before().total_tokens,
                                val.current().total_tokens,
                                val.compact_ratio(),
                            ),
                            HttpChannel::create_resp_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(client, session_id, agent_id).await;
                        }
                    }
                    HistoryCompactResult::Err(err_msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            agent,
                            session_id,
                            &self.ctx,
                            AgentRespType::HistoryCompactErr,
                            inbound_message,
                            err_msg.clone(),
                            HttpChannel::create_resp_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(client, session_id, agent_id).await;
                        }
                    }
                    HistoryCompactResult::Ignore(msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            agent,
                            session_id,
                            &self.ctx,
                            AgentRespType::HistoryCompactIgnore,
                            inbound_message,
                            format!(
                                r#"### 压缩请求被忽略
{msg}
"#
                            ),
                            HttpChannel::create_resp_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(client, session_id, agent_id).await;
                        }
                    }
                }

                Ok(curr_state)
            }
        }
    }
}

impl HttpChannel {
    fn create_resp_messages(
        agent: &dyn Agent,
        session_id: &SessionId,
        _: &ChannelContext,
        input: Option<&HttpReqMessage>,
        content: String,
    ) -> crate::Result<HttpRespMessage> {
        let output = if let Some(_) = &agent.agent_settings().output_schema {
            let json = serde_json::from_str(&content)?;
            Payload::Json(json)
        } else {
            Payload::Text(content.into())
        };
        let message = match &session_id {
            SessionId::Master { .. } | SessionId::Anonymous { .. } => HttpRespMessage {
                output,
                input: input.map(|it| it.clone()),
            },
            SessionId::Group { .. } => {
                unreachable!("send robot message to group is not supported by http")
            }
        };
        Ok(message)
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
