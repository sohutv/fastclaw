use super::super::{AgentRespState, AgentRespType};
use crate::agent::{AgentResponse, HistoryCompactResult, Notify};
use crate::channels::wechat_channel::WechatChannel;
use crate::channels::{ChannelMessage, create_robot_messages_for_agent};
use anyhow::anyhow;
use rig::completion::{AssistantContent, Message};
use rig::message::{ReasoningContent, ToolCall, ToolFunction};
use wechat_sdk::client::WechatClient;
use wechat_sdk::client::message::TypingTicket;

impl WechatChannel {
    pub(super) async fn handle_agent_message_actual(
        &self,
        wechat: &WechatClient,
        typing_ticket: Option<&TypingTicket>,
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
                    if let Some(typing_ticket) = typing_ticket {
                        let _ = wechat.send_typing(&typing_ticket).await;
                    }
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
                    WechatChannel::create_robot_messages,
                )
                .await
                {
                    let _ = robot_message.send(&wechat).await;
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
                                content,
                                WechatChannel::create_robot_messages,
                            )
                            .await?
                            {
                                let _ = robot_message.send(&wechat).await;
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
                    content,
                    WechatChannel::create_robot_messages,
                )
                .await?
                {
                    let _ = robot_message.send(&wechat).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Error(error) => {
                if let Some(robot_message) = create_robot_messages_for_agent(
                    session_id,
                    &self.ctx,
                    AgentRespType::Error,
                    format!("Agent error: {}", error),
                    WechatChannel::create_robot_messages,
                )
                .await?
                {
                    let _ = robot_message.send(&wechat).await;
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
                            text,
                            WechatChannel::create_robot_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(&wechat).await;
                        }
                    }
                    Notify::Markdown { content, .. } => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::Notify,
                            format!("{content}",),
                            WechatChannel::create_robot_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(&wechat).await;
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
                            &format!(
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
                            WechatChannel::create_robot_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(&wechat).await;
                        }
                    }
                    HistoryCompactResult::Err(err_msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::HistoryCompactErr,
                            err_msg,
                            WechatChannel::create_robot_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(&wechat).await;
                        }
                    }
                    HistoryCompactResult::Ignore(msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::HistoryCompactIgnore,
                            format!(
                                r#"
### 压缩请求被忽略
{msg}
                            "#
                            ),
                            WechatChannel::create_robot_messages,
                        )
                        .await?
                        {
                            let _ = robot_message.send(&wechat).await;
                        }
                    }
                }

                Ok(curr_state)
            }
        }
    }
}
