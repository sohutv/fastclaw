use crate::agent::{AgentResponse, HistoryCompactResult, Notify};
use crate::channels::dingtalk_channel::DingtalkChannel;
use crate::channels::{ChannelContext, ChannelMessage, SessionId, SessionSettings};
use anyhow::anyhow;
use dingtalk_stream::DingTalkStream;
use dingtalk_stream::frames::up_message::robot_message::RobotMessage;
use dingtalk_stream::frames::up_message::{
    MessageContent, MessageContentMarkdown, MessageContentText,
};
use rig::completion::{AssistantContent, Message};
use rig::message::{ReasoningContent, ToolCall, ToolFunction};
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

impl DingtalkChannel {
    pub async fn recv_agent_message(
        dingtalk: Arc<DingTalkStream>,
        ctx: &ChannelContext,
        receiver: &mut Receiver<ChannelMessage>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut buff = Vec::<String>::new();
        while let Some(message) = receiver.recv().await {
            match Self::handle_agent_message(&dingtalk, &*ctx, &message, state, &mut buff).await {
                Ok(AgentRespState::Final) | Err(_) => {
                    state = AgentRespState::Wait;
                    buff.clear();
                }
                Ok(next) => {
                    state = next;
                }
            }
        }
        Ok(())
    }

    async fn handle_agent_message(
        dingtalk: &DingTalkStream,
        ctx: &ChannelContext,
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
                    if let Some(robot_message) = create_robot_messages_for_agent(
                        session_id,
                        ctx,
                        AgentRespType::Start,
                        MessageContentText::from("正在思考..."),
                    )
                    .await
                    {
                        let _ = dingtalk.send_message(robot_message).await;
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
                    MessageContentMarkdown::from((
                        format!("工具调用: {name}..."),
                        format!(
                            r#"
### 工具调用: {name}...
```
{}
```json
                                            "#,
                            serde_json::to_string_pretty(arguments).unwrap_or_else(|err| format!(
                                "Error serializing arguments: {}",
                                err
                            ))
                        ),
                    )),
                )
                .await
                {
                    let _ = dingtalk.send_message(robot_message).await;
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
                                MessageContentMarkdown::from((
                                    "正在思考...",
                                    format!(
                                        r#"
### 我的想法..
{content}
                                    "#
                                    ),
                                ))
                            };
                            if let Some(robot_message) = create_robot_messages_for_agent(
                                session_id,
                                ctx,
                                AgentRespType::Reasoning,
                                content,
                            )
                            .await
                            {
                                let _ = dingtalk.send_message(robot_message).await;
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
                    let content = MessageContentMarkdown::from((
                        "回复中...",
                        format!(
                            r#"
{}

*<<Tokens:{}↑{}↓{}>>*
                    "#,
                            buff.join(""),
                            usage.total_tokens,
                            usage.input_tokens,
                            usage.output_tokens
                        ),
                    ));
                    buff.clear();
                    content
                };
                if let Some(robot_message) = create_robot_messages_for_agent(
                    session_id,
                    ctx,
                    AgentRespType::Content,
                    content,
                )
                .await
                {
                    let _ = dingtalk.send_message(robot_message).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Error(error) => {
                if let Some(robot_message) = create_robot_messages_for_agent(
                    session_id,
                    ctx,
                    AgentRespType::Error,
                    MessageContentText::from(format!("Agent error: {}", error)),
                )
                .await
                {
                    let _ = dingtalk.send_message(robot_message).await;
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
                            MessageContentText::from(text),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    Notify::Markdown { title, content } => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::Notify,
                            MessageContentMarkdown::from((title, &format!("{content}",))),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
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
                            MessageContentMarkdown::from((
                                "压缩上下文完成",
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
                            )),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    HistoryCompactResult::Err(err_msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::HistoryCompactErr,
                            MessageContentText::from(err_msg),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    HistoryCompactResult::Ignore(msg) => {
                        if let Some(robot_message) = create_robot_messages_for_agent(
                            session_id,
                            ctx,
                            AgentRespType::HistoryCompactIgnore,
                            MessageContentMarkdown::from((
                                "压缩请求被忽略",
                                format!(
                                    r#"
### 压缩请求被忽略
{msg}
                            "#
                                ),
                            )),
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
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

async fn create_robot_messages_for_agent<Content: Into<MessageContent>>(
    session_id: &SessionId,
    ctx: &ChannelContext,
    resp_type: AgentRespType,
    content: Content,
) -> Option<RobotMessage> {
    let Some(session_id) = ctx
        .config
        .dingtalk_config
        .as_ref()
        .and_then(|cfg| SessionId::try_from((session_id.deref(), cfg)).ok())
    else {
        return None;
    };

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
                return None;
            };
        }
        AgentRespType::ToolCall => {
            let true = show_toolcall else {
                return None;
            };
        }
        AgentRespType::Reasoning => {
            let true = show_reasoning else {
                return None;
            };
        }
        AgentRespType::Content => {}
        AgentRespType::Notify => {
            let true = show_notify else {
                return None;
            };
        }
        AgentRespType::HistoryCompactOk => {
            let true = (*show_compacting && *show_compacting_ok) else {
                return None;
            };
        }
        AgentRespType::HistoryCompactErr => {
            let true = (*show_compacting && *show_compacting_err) else {
                return None;
            };
        }
        AgentRespType::HistoryCompactIgnore => {
            let true = (*show_compacting && *show_compacting_ignore) else {
                return None;
            };
        }
        AgentRespType::Error => {
            let true = show_error else {
                return None;
            };
        }
    }
    DingtalkChannel::create_robot_messages(&session_id, ctx, content).await
}

#[derive(Debug, Clone, Copy)]
enum AgentRespState {
    Wait,
    Start,
    Reasoning,
    Messaging,
    Final,
}
