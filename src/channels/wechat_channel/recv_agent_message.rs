use crate::agent::{AgentResponse, HistoryCompactResult, Notify};
use crate::channels::wechat_channel::WechatChannel;
use crate::channels::{ChannelContext, ChannelMessage, SessionId, SessionSettings};
use anyhow::anyhow;
use rig::completion::{AssistantContent, Message};
use rig::message::{ReasoningContent, ToolCall, ToolFunction};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use wechat_sdk::client::WechatClient;
use wechat_sdk::client::message::{MessageItems, TypingTicket};

impl WechatChannel {
    pub async fn recv_agent_message(
        wechat: Arc<WechatClient>,
        ctx: &ChannelContext,
        receiver: &mut Receiver<ChannelMessage>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        let config = ctx
            .config
            .wechat_config
            .as_ref()
            .expect("unexpected wechat_config");
        let session_id = &config.session_id;
        let typing_ticket = wechat.get_config(session_id.to_string(), None).await.ok();
        while let Some(message) = receiver.recv().await {
            match Self::handle_agent_message(
                &wechat,
                ctx,
                typing_ticket.as_ref(),
                &message,
                state,
                &mut buff,
            )
            .await
            {
                Ok(AgentRespState::Final) | Err(_) => {
                    state = AgentRespState::Wait;
                    buff.clear();
                }
                Ok(next) => {
                    state = next;
                }
            }
        }
        if let Some(typing_ticket) = typing_ticket {
            let _ = wechat
                .send_typing_cannel(session_id.to_string(), &typing_ticket)
                .await;
        }
        Ok(())
    }

    async fn handle_agent_message(
        wechat: &WechatClient,
        ctx: &ChannelContext,
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
                        let _ = wechat
                            .send_typing(session_id.to_string(), &typing_ticket)
                            .await;
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
                if let Some(robot_message) = create_robot_messages_for_agent(
                    session_id,
                    ctx,
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
                )
                .await?
                {
                    let _ = robot_message.send(&wechat).await;
                }
                Ok(curr_state)
            }
            AgentResponse::ReasoningStream(reasoning) => {
                match curr_state {
                    AgentRespState::Start => if ctx.config.default_show_reasoning {},
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
                        if ctx.config.default_show_reasoning {
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
                                ctx,
                                AgentRespType::Reasoning,
                                content,
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
                    ctx,
                    AgentRespType::Content,
                    content,
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
                    ctx,
                    AgentRespType::Error,
                    format!("Agent error: {}", error),
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
                            ctx,
                            AgentRespType::Notify,
                            text,
                        )
                        .await?
                        {
                            let _ = robot_message.send(&wechat).await;
                        }
                    }
                    Notify::Markdown { content, .. } => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::Notify,
                            format!("{content}",),
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
                            ctx,
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
                        )
                        .await?
                        {
                            let _ = robot_message.send(&wechat).await;
                        }
                    }
                    HistoryCompactResult::Err(err_msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::HistoryCompactErr,
                            err_msg,
                        )
                        .await?
                        {
                            let _ = robot_message.send(&wechat).await;
                        }
                    }
                    HistoryCompactResult::Ignore(msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::HistoryCompactIgnore,
                            format!(
                                r#"
### 压缩请求被忽略
{msg}
                            "#
                            ),
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

pub enum AgentRespType {
    Start,
    ToolCall,
    Reasoning,
    Content,
    Notify,
    HistoryCompactOk,
    HistoryCompactErr,
    HistoryCompactIgnore,
    Error,
}

async fn create_robot_messages_for_agent<'a, Content: Into<MessageItems>>(
    session_id: &'a SessionId,
    ctx: &ChannelContext,
    resp_type: AgentRespType,
    content: Content,
) -> crate::Result<Option<super::WechatRobotMessage<'a>>> {
    let SessionSettings {
        show_start,
        show_toolcall,
        show_reasoning,
        show_notify,
        show_compacting,
        show_compacting_ok,
        show_compacting_err,
        show_compacting_ignore,
        show_error,
        ..
    } = session_id.settings();
    match resp_type {
        AgentRespType::Start => {
            let true = show_start else {
                return Ok(None);
            };
        }
        AgentRespType::ToolCall => {
            let true = show_toolcall else {
                return Ok(None);
            };
        }
        AgentRespType::Reasoning => {
            let true = show_reasoning else {
                return Ok(None);
            };
        }
        AgentRespType::Content => {}
        AgentRespType::Notify => {
            let true = show_notify else {
                return Ok(None);
            };
        }
        AgentRespType::HistoryCompactOk => {
            let true = (*show_compacting && *show_compacting_ok) else {
                return Ok(None);
            };
        }
        AgentRespType::HistoryCompactErr => {
            let true = (*show_compacting && *show_compacting_err) else {
                return Ok(None);
            };
        }
        AgentRespType::HistoryCompactIgnore => {
            let true = (*show_compacting && *show_compacting_ignore) else {
                return Ok(None);
            };
        }
        AgentRespType::Error => {
            let true = show_error else {
                return Ok(None);
            };
        }
    }
    Ok(Some(
        WechatChannel::create_robot_messages(session_id, ctx, content).await?,
    ))
}

#[derive(Debug, Clone, Copy)]
enum AgentRespState {
    Wait,
    Start,
    Reasoning,
    Messaging,
    Final,
}
