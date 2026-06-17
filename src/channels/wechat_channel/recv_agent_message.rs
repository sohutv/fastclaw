use super::super::{AgentRespState, AgentRespType};
use crate::agent::{Agent, AgentResponse, Notify};
use crate::channels::text_formater::*;
use crate::channels::wechat_channel::WechatChannel;
use crate::channels::{ChannelContext, ChannelMessage, SessionId, create_robot_messages_for_agent};
use anyhow::anyhow;
use wechat_sdk::client::WechatClient;
use wechat_sdk::client::message::{MessageItems, TypingTicket};

impl WechatChannel {
    pub(super) async fn handle_agent_message_actual(
        &self,
        wechat: &WechatClient,
        typing_ticket: Option<&TypingTicket>,
        ChannelMessage {
            session_id,
            agent_id: _,
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
                if let Some(typing_ticket) = typing_ticket {
                    let _ = wechat.send_typing(&typing_ticket).await;
                }
                (None, AgentRespState::Start)
            }
            AgentResponse::ToolCall(toolcall) => (
                format_tool_call(session_id, &self.wechat_config, toolcall),
                curr_state,
            ),
            AgentResponse::ReasoningStream(reasoning) => {
                buff.extend(extract_reasoning(
                    session_id,
                    &self.wechat_config,
                    reasoning,
                ));
                (None, AgentRespState::Reasoning)
            }
            AgentResponse::MessageStream(message) => {
                let formated_message = if let AgentRespState::Reasoning = curr_state {
                    Some(format_reasoning(session_id, buff))
                } else {
                    None
                };
                buff.extend(extract_message(session_id, &self.wechat_config, message));
                (formated_message, AgentRespState::Messaging)
            }
            AgentResponse::Final(usage) => (
                Some(
                    format_message(
                        session_id,
                        &self.wechat_config,
                        self.agent.agent_settings().output_schema.is_some(),
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
                Some(format_history_compact(
                    session_id,
                    &self.wechat_config,
                    result,
                )),
                curr_state,
            ),
        };
        if let Some((text, resp_type)) = formated_message {
            if let Some(robot_message) = create_robot_messages_for_agent(
                &*self.agent,
                session_id,
                &self.wechat_config,
                &self.ctx,
                resp_type,
                text,
                WechatChannel::create_robot_messages,
            )
            .await?
            {
                let _ = robot_message.send(&wechat).await;
            }
        }
        Ok(next_state)
    }
}

impl WechatChannel {
    fn create_robot_messages<Content: Into<MessageItems>>(
        _: &dyn Agent,
        session_id: &SessionId,
        _: &ChannelContext,
        content: Content,
    ) -> crate::Result<WechatRobotMessage> {
        let message = match &session_id {
            SessionId::Master { .. } | SessionId::Anonymous { .. } => WechatRobotMessage {
                content: content.into(),
            },
            SessionId::Group { .. } => {
                unreachable!("send robot message to group is not supported by wechat")
            }
        };
        Ok(message)
    }
}

struct WechatRobotMessage {
    content: MessageItems,
}

impl WechatRobotMessage {
    async fn send(self, wechat: &WechatClient) -> crate::Result<()> {
        let _ = wechat.send_message(self.content).await?;
        Ok(())
    }
}
