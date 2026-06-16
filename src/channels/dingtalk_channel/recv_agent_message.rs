use super::super::{AgentRespState, AgentRespType};
use crate::agent::{AgentResponse, Notify};
use crate::channels::ChannelMessage;
use crate::channels::dingtalk_channel::DingtalkChannel;
use crate::channels::text_formater::{
    extract_message, extract_reasoning, format_history_compact, format_message, format_reasoning,
    format_tool_call,
};
use anyhow::anyhow;
use dingtalk_stream::DingTalkStream;
use dingtalk_stream::frames::down_message::callback_message::MessageData;
use dingtalk_stream::frames::up_message::{
    MessageContent, MessageContentMarkdown, MessageContentText,
};

impl DingtalkChannel {
    pub(super) async fn handle_agent_message_actual(
        &self,
        dingtalk: &DingTalkStream,
        inbound_message: Option<&MessageData>,
        ChannelMessage {
            session_id,
            agent_id: _,
            message,
        }: &ChannelMessage,
        curr_state: AgentRespState,
        buff: &mut Vec<String>,
    ) -> crate::Result<AgentRespState> {
        let (formated_message, next_state): (Option<(MessageContent, _)>, _) = match message {
            AgentResponse::Start => {
                let AgentRespState::Wait = curr_state else {
                    return Err(anyhow!("AgentRespState must be Init when starting"));
                };
                buff.clear();
                (
                    Some((
                        MessageContentText::from("正在思考...").into(),
                        AgentRespType::Start,
                    )),
                    AgentRespState::Start,
                )
            }
            AgentResponse::ToolCall(toolcall) => (
                format_tool_call(session_id, &self.dingtalk_config, toolcall).map(|(text, rt)| {
                    (
                        MessageContentMarkdown::from((
                            format!("工具调用: {}...", toolcall.function.name),
                            text,
                        ))
                        .into(),
                        rt,
                    )
                }),
                curr_state,
            ),
            AgentResponse::ReasoningStream(reasoning) => {
                buff.extend(extract_reasoning(
                    session_id,
                    &self.dingtalk_config,
                    reasoning,
                ));
                (None, AgentRespState::Reasoning)
            }
            AgentResponse::MessageStream(message) => {
                let formated_message = if let AgentRespState::Reasoning = curr_state {
                    let (text, rt) = format_reasoning(session_id, buff);
                    Some((
                        MessageContentMarkdown::from(("正在思考...", text)).into(),
                        rt,
                    ))
                } else {
                    None
                };
                buff.extend(extract_message(session_id, &self.dingtalk_config, message));
                (formated_message, AgentRespState::Messaging)
            }
            AgentResponse::Final(usage) => {
                let (text, rt) = format_message(
                    session_id,
                    &self.dingtalk_config,
                    self.agent.agent_settings().output_schema.is_some(),
                    usage,
                    buff,
                )
                .map(|(msg, rt)| (msg.to_string(), rt))?;
                (
                    Some((MessageContentMarkdown::from(("回复中...", text)).into(), rt)),
                    AgentRespState::Final,
                )
            }
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
                        Notify::Text(text) => text.into(),
                        Notify::Markdown { title, content, .. } => {
                            MessageContentMarkdown::from((title, &format!("{content}",))).into()
                        }
                    },
                    AgentRespType::Notify,
                )),
                curr_state,
            ),
            AgentResponse::HistoryCompact(result) => {
                let (text, rt) = format_history_compact(session_id, &self.dingtalk_config, result);
                (
                    Some((
                        MessageContentMarkdown::from(("压缩上下文", text)).into(),
                        rt,
                    )),
                    curr_state,
                )
            }
        };
        if let Some((message_content, resp_type)) = formated_message {
            if let Ok(Some(robot_message)) = super::super::create_robot_messages_for_agent(
                &*self.agent,
                session_id,
                &self.dingtalk_config,
                &self.ctx,
                resp_type,
                inbound_message,
                message_content,
                DingtalkChannel::create_robot_messages,
            )
            .await
            {
                let _ = dingtalk.send_message(robot_message).await;
            }
        }
        Ok(next_state)
    }
}
