use crate::agent::{AgentResponse, HistoryCompactResult, Notify};
use crate::channels::http_channel::type_::HttpReqMessage;
use crate::channels::http_channel::{Client, HttpChannel};
use crate::channels::http_channel::{HttpRespMessage, Payload, UserId};
use crate::channels::{
    AgentRespState, AgentRespType, ChannelContext, ChannelMessage, SessionId,
    create_robot_messages_for_agent,
};
use anyhow::anyhow;
use rig::completion::{AssistantContent, Message};
use rig::message::{ReasoningContent, ToolCall, ToolFunction};
use std::mem;

impl HttpChannel {
    pub(super) async fn handle_agent_message_actual(
        &self,
        client: &Client,
        inbound_message: Option<&HttpReqMessage>,
        ChannelMessage {
            session_id,
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
                    session_id,
                    &self.ctx,
                    AgentRespType::ToolCall,
                    inbound_message,
                    format!(
                        r#"
### 工具调用: {name}...
```
{}
```json
                                            "#,
                        serde_json::to_string_pretty(arguments)
                            .unwrap_or_else(|err| format!("Error serializing arguments: {}", err))
                    ),
                    HttpChannel::create_resp_messages,
                )
                .await
                {
                    let _ = robot_message.send(client, inbound_message).await;
                }
                Ok(curr_state)
            }
            AgentResponse::ReasoningStream(reasoning) => {
                match curr_state {
                    AgentRespState::Start => if self.ctx.config.default_show_reasoning {},
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
                        if self.ctx.config.default_show_reasoning {
                            let content = {
                                let content = buff.join("");
                                buff.clear();
                                format!(
                                    r#"
### 我的想法..
{content}
                                    "#
                                )
                            };
                            if let Some(robot_message) = create_robot_messages_for_agent(
                                session_id,
                                &self.ctx,
                                AgentRespType::Reasoning,
                                inbound_message,
                                content,
                                HttpChannel::create_resp_messages,
                            )
                            .await?
                            {
                                let _ = robot_message.send(client, inbound_message).await;
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
                    let content = format!(
                        r#"
{}

*<<Tokens:{}↑{}↓{}>>*
                    "#,
                        buff.join(""),
                        usage.total_tokens,
                        usage.input_tokens,
                        usage.output_tokens
                    );
                    buff.clear();
                    content
                };
                if let Some(robot_message) = create_robot_messages_for_agent(
                    session_id,
                    &self.ctx,
                    AgentRespType::Content,
                    inbound_message,
                    content,
                    HttpChannel::create_resp_messages,
                )
                .await?
                {
                    let _ = robot_message.send(client, inbound_message).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Error(error) => {
                if let Some(robot_message) = create_robot_messages_for_agent(
                    session_id,
                    &self.ctx,
                    AgentRespType::Error,
                    inbound_message,
                    format!("Agent error: {}", error),
                    HttpChannel::create_resp_messages,
                )
                .await?
                {
                    let _ = robot_message.send(client, inbound_message).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Notify(notify) => {
                match notify {
                    Notify::Text(text) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::Notify,
                            inbound_message,
                            text.clone(),
                            HttpChannel::create_resp_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(client, inbound_message).await;
                        }
                    }
                    Notify::Markdown { content, .. } => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::Notify,
                            inbound_message,
                            format!("{content}",),
                            HttpChannel::create_resp_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(client, inbound_message).await;
                        }
                    }
                }
                Ok(curr_state)
            }
            AgentResponse::HistoryCompact(result) => {
                match result {
                    HistoryCompactResult::Ok(val) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::HistoryCompactOk,
                            inbound_message,
                            format!(
                                r#"
### 压缩上下文完成
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
                            let _ = robot_message.send(client, inbound_message).await;
                        }
                    }
                    HistoryCompactResult::Err(err_msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::HistoryCompactErr,
                            inbound_message,
                            err_msg.clone(),
                            HttpChannel::create_resp_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(client, inbound_message).await;
                        }
                    }
                    HistoryCompactResult::Ignore(msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::HistoryCompactIgnore,
                            inbound_message,
                            format!(
                                r#"
### 压缩请求被忽略
{msg}
                            "#
                            ),
                            HttpChannel::create_resp_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(client, inbound_message).await;
                        }
                    }
                }

                Ok(curr_state)
            }
        }
    }
}

impl HttpChannel {
    fn create_resp_messages<Content: Into<Payload>>(
        session_id: &SessionId,
        _: &ChannelContext,
        inbound: Option<&HttpReqMessage>,
        content: Content,
    ) -> crate::Result<HttpRespMessage> {
        let message = match &session_id {
            SessionId::Master { .. } | SessionId::Anonymous { .. } => HttpRespMessage {
                message_id: inbound.map(|it| it.message_id.clone()).unwrap_or_default(),
                user_id: UserId::from(session_id),
                payloads: vec![content.into()],
            },
            SessionId::Group { .. } => {
                unreachable!("send robot message to group is not supported by http")
            }
        };
        Ok(message)
    }
}

impl HttpRespMessage {
    async fn send(self, client: &Client, inbound: Option<&HttpReqMessage>) {
        if let Some(inbound) = inbound {
            let mut client = client.lock().await;
            if let Some(transports) = client.get_mut(&inbound.user_id) {
                let mut updated = vec![];
                for transport in mem::replace(transports, vec![]) {
                    {
                        if let Some(sender) = transport.sender.upgrade() {
                            if !sender.is_closed() {
                                let _ = sender.send(self.clone()).await;
                                updated.push(transport);
                            }
                        }
                    }
                }
                *transports = updated;
            }
        }
    }
}
