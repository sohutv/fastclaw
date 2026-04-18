use super::super::{AgentRespState, AgentRespType};
use crate::agent::{AgentResponse, HistoryCompactResult, Notify};
use crate::channels::dingtalk_channel::DingtalkChannel;
use crate::channels::{ChannelContext, ChannelMessage, SessionId};
use anyhow::anyhow;
use dingtalk_stream::DingTalkStream;
use dingtalk_stream::frames::up_message::{MessageContentMarkdown, MessageContentText};
use rig::completion::{AssistantContent, Message};
use rig::message::{ReasoningContent, ToolCall, ToolFunction};
use std::ops::Deref;

impl DingtalkChannel {
    pub(super) async fn handle_agent_message(
        &self,
        dingtalk: &DingTalkStream,
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
                    if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                        session_id,
                        &self.ctx,
                        AgentRespType::Start,
                        MessageContentText::from("正在思考..."),
                        DingtalkChannel::create_robot_messages,
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
                if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                    session_id,
                    &self.ctx,
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
                    DingtalkChannel::create_robot_messages,
                )
                .await
                {
                    let _ = dingtalk.send_message(robot_message).await;
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
                            if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                                session_id,
                                &self.ctx,
                                AgentRespType::Reasoning,
                                content,
                                DingtalkChannel::create_robot_messages,
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
                if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                    session_id,
                    &self.ctx,
                    AgentRespType::Content,
                    content,
                    DingtalkChannel::create_robot_messages,
                )
                .await
                {
                    let _ = dingtalk.send_message(robot_message).await;
                }
                Ok(AgentRespState::Final)
            }
            AgentResponse::Error(error) => {
                if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                    session_id,
                    &self.ctx,
                    AgentRespType::Error,
                    MessageContentText::from(format!("Agent error: {}", error)),
                    DingtalkChannel::create_robot_messages,
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
                        if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::Notify,
                            MessageContentText::from(text),
                            DingtalkChannel::create_robot_messages,
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    Notify::Markdown { title, content } => {
                        if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::Notify,
                            MessageContentMarkdown::from((title, &format!("{content}",))),
                            DingtalkChannel::create_robot_messages,
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
                        if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
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
                            DingtalkChannel::create_robot_messages,
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    HistoryCompactResult::Err(err_msg) => {
                        if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
                            AgentRespType::HistoryCompactErr,
                            MessageContentText::from(err_msg),
                            DingtalkChannel::create_robot_messages,
                        )
                        .await
                        {
                            let _ = dingtalk.send_message(robot_message).await;
                        }
                    }
                    HistoryCompactResult::Ignore(msg) => {
                        if let Ok(Some(robot_message)) = create_robot_messages_for_agent(
                            session_id,
                            &self.ctx,
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
                            DingtalkChannel::create_robot_messages,
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

async fn create_robot_messages_for_agent<Content, F, OutboundMsg>(
    session_id: &SessionId,
    ctx: &ChannelContext,
    resp_type: AgentRespType,
    content: Content,
    outbound_msg_creator: F,
) -> crate::Result<Option<OutboundMsg>>
where
    F: FnOnce(&SessionId, &ChannelContext, Content) -> crate::Result<OutboundMsg>,
{
    let Some(session_id) = ctx
        .config
        .dingtalk_config
        .as_ref()
        .and_then(|cfg| SessionId::try_from((session_id.deref(), cfg)).ok())
    else {
        return Ok(None);
    };
    super::super::create_robot_messages_for_agent(
        &session_id,
        ctx,
        resp_type,
        content,
        outbound_msg_creator,
    )
    .await
}
