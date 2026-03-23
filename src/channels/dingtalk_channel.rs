use crate::agent::{AgentMessageSender, AgentSignal};
use crate::channels::console_cmd::Console;
use crate::channels::{ChannelContext, ChannelMessageSender};
use crate::config::{Config, DingTalkConfig};
use anyhow::anyhow;
use async_trait::async_trait;
use dingtalk_stream::client::DingtalkMessageSender;
use dingtalk_stream::frames::{
    CallbackMessageData, CallbackMessagePayload, CallbackWebhookMessage, RichTextItem,
    RobotBatchMessage, UpMessageContentMarkdown,
};
use dingtalk_stream::{CallbackMessage, DingTalkStream, Error, ErrorCode, MessageTopic, Resp};
use itertools::Itertools;
use rig::completion::{AssistantContent, Message};
use rig::message::{ReasoningContent, ToolCall, ToolFunction};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::RwLock;
use tokio::sync::mpsc::{Receiver, Sender};

pub struct DingtalkChannel {
    ctx: Arc<RwLock<ChannelContext>>,
    agent_signal_receiver: Receiver<AgentSignal>,
    agent_message_sender: AgentMessageSender,
    dingtalk_config: DingTalkConfig,
}

impl DingtalkChannel {
    pub fn new(
        config: &'static Config,
        agent_message_sender: AgentMessageSender,
    ) -> crate::Result<(Self, ChannelMessageSender)> {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        Ok((
            Self {
                ctx: Arc::new(RwLock::new(ChannelContext {
                    config: config.clone(),
                })),
                agent_signal_receiver: receiver,
                agent_message_sender,
                dingtalk_config: config
                    .dingtalk_config
                    .clone()
                    .ok_or(anyhow!("dingtalk config not found"))?,
            },
            sender.into(),
        ))
    }
}

#[allow(unused)]
struct DingTalkCallbackHandler {
    ctx: Arc<RwLock<ChannelContext>>,
    dingtalk_config: DingTalkConfig,
    dingtalk_bot_topic: MessageTopic,
    agent_message_sender: AgentMessageSender,
}

#[async_trait]
impl dingtalk_stream::CallbackHandler for DingTalkCallbackHandler {
    async fn process(
        &self,
        CallbackMessage { data, .. }: &CallbackMessage,
        _cb_msg_sender: Option<Sender<CallbackWebhookMessage>>,
    ) -> Result<Resp, Error> {
        let Some(CallbackMessageData {
            msg_id: _,
            payload: Some(payload),
            ..
        }) = data
        else {
            return Err(Error {
                code: ErrorCode::BadRequest,
                msg: "unexpected data".to_string(),
            });
        };
        let line = match payload {
            CallbackMessagePayload::Text { text } => text.content.to_string(),
            CallbackMessagePayload::Picture { .. } => "".to_string(),
            CallbackMessagePayload::File { .. } => "".to_string(),
            CallbackMessagePayload::RichText { content } => content
                .content
                .iter()
                .map(|it| match it {
                    RichTextItem::Picture { .. } => "".to_string(),
                    RichTextItem::Text { text } => text.to_string(),
                })
                .join(""),
        };
        let line = line.trim();
        if !line.is_empty() {
            if line.starts_with('/') {
                Console::handle_console_cmd(Arc::clone(&self.ctx), &line).await;
            } else {
                let message = Message::user(line);
                let _ = self.agent_message_sender.send(message).await;
            }
        }
        Ok(Resp::Text(format!("echo {}", line)))
    }

    fn topic(&self) -> &MessageTopic {
        &self.dingtalk_bot_topic
    }
}

impl DingtalkChannel {
    pub async fn start(self) -> crate::Result<JoinHandle<()>> {
        let Self {
            ctx,
            agent_signal_receiver: receiver,
            agent_message_sender,
            dingtalk_config,
        } = self;
        let cb_handler = {
            let ctx = Arc::clone(&ctx);
            DingTalkCallbackHandler {
                ctx,
                dingtalk_config: dingtalk_config.clone(),
                dingtalk_bot_topic: MessageTopic::Callback(
                    dingtalk_stream::TOPIC_ROBOT.to_string(),
                ),
                agent_message_sender,
            }
        };
        let (mut dingtalk_stream, dingtalk_msg_sender) =
            DingTalkStream::new(dingtalk_config.credential)
                .register_callback_handler(cb_handler)
                .create_message_sender()
                .await;
        let join_handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("unexpected err");
            let dingtalk_stream_handle = {
                rt.spawn(async move {
                    let stop_tx = Arc::clone(&dingtalk_stream.stop_tx);
                    tokio::spawn(async move {
                        dingtalk_stream.start_forever().await;
                    });
                    let _ = tokio::signal::ctrl_c().await;
                    let stop_tx = stop_tx.lock().await;
                    if let Some(stop_tx) = stop_tx.as_ref() {
                        let _ = stop_tx.send(()).await;
                    }
                })
            };
            let agent_handle = {
                let ctx = Arc::clone(&ctx);
                rt.spawn(async move {
                    let mut receiver = receiver;
                    let _ = DingtalkChannel::poll_agent_signal(
                        &ctx,
                        &mut receiver,
                        &dingtalk_msg_sender,
                    )
                    .await;
                })
            };
            rt.block_on(async {
                let _ = dingtalk_stream_handle.await;
                let _ = agent_handle.await;
            });
        });
        Ok(join_handle)
    }
}

impl DingtalkChannel {
    async fn poll_agent_signal(
        ctx: &Arc<RwLock<ChannelContext>>,
        receiver: &mut Receiver<AgentSignal>,
        dingtalk_msg_sender: &DingtalkMessageSender,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Wait;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        let mut buff = Vec::<String>::new();
        loop {
            tokio::select! {
                message = receiver.recv() => {
                    if let Some(signal) = message {
                        let ctx = ctx.read().await;
                        match  Self::handle_agent_signal(&*ctx, &signal, state, &mut buff, &dingtalk_msg_sender).await{
                            Ok(AgentRespState::Final) | Err( _)=> {
                                state = AgentRespState::Wait;
                                buff.clear();
                            },
                            Ok(next)=>{
                                state = next;
                            }
                        }
                    } else {
                        return Ok(());
                    }
                },
                _ = interval.tick() => {
                    match state{
                        AgentRespState::Wait|AgentRespState::Start => {
                           //todo!()
                        }
                        _=>{}
                    }
                },
                _ = tokio::signal::ctrl_c() => {
                    return Ok(());
                }
            }
        }
    }

    async fn handle_agent_signal(
        ctx: &ChannelContext,
        signal: &AgentSignal,
        curr_state: AgentRespState,
        buff: &mut Vec<String>,
        dingtalk_msg_sender: &DingtalkMessageSender,
    ) -> crate::Result<AgentRespState> {
        match signal {
            AgentSignal::Start => {
                if let AgentRespState::Wait = curr_state {
                    buff.clear();
                    let _ = dingtalk_msg_sender
                        .send(RobotBatchMessage {
                            user_ids: vec![USER_ID.into()],
                            content: UpMessageContentMarkdown::from(("思考中...", "正在思考..."))
                                .into(),
                            send_result_cb: None,
                        })
                        .await;
                    Ok(AgentRespState::Start)
                } else {
                    Err(anyhow!("AgentRespState must be Init when starting"))
                }
            }
            AgentSignal::ToolCall(ToolCall {
                function: ToolFunction { name, arguments },
                ..
            }) => {
                let _ = dingtalk_msg_sender
                    .send(RobotBatchMessage {
                        user_ids: vec![USER_ID.into()],
                        content: UpMessageContentMarkdown::from((
                            format!("工具调用: {name}"),
                            format!(
                                r#"
### 工具调用: {name}
```
{}
```json
                                            "#,
                                serde_json::to_string_pretty(arguments).unwrap_or_else(
                                    |err| format!("Error serializing arguments: {}", err)
                                )
                            ),
                        ))
                        .into(),
                        send_result_cb: None,
                    })
                    .await;
                Ok(curr_state)
            }
            AgentSignal::ReasoningStream(reasoning) => {
                match curr_state {
                    AgentRespState::Start => if ctx.config.show_reasoning {},
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
            AgentSignal::MessageStream(message) => {
                match curr_state {
                    AgentRespState::Start => {}
                    AgentRespState::Reasoning => {
                        if ctx.config.show_reasoning {
                            let content = {
                                let string = buff.join("");
                                buff.clear();
                                string
                            };
                            let _ = dingtalk_msg_sender
                                .send(RobotBatchMessage {
                                    user_ids: vec![USER_ID.into()],
                                    content: UpMessageContentMarkdown::from((
                                        "正在思考...",
                                        format!(
                                            r#"
### 正在思考...
{content}
                                    "#
                                        ),
                                    ))
                                    .into(),
                                    send_result_cb: None,
                                })
                                .await;
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
            AgentSignal::Final(usage) => {
                let content = {
                    let content = UpMessageContentMarkdown::from((
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
                let _ = dingtalk_msg_sender
                    .send(RobotBatchMessage {
                        user_ids: vec![USER_ID.into()],
                        content: content.into(),
                        send_result_cb: None,
                    })
                    .await;
                Ok(AgentRespState::Final)
            }
            AgentSignal::Error(error) => {
                eprintln!("{}", error);
                Err(anyhow!("Agent error: {}", error))
            }
        }
    }
}

const USER_ID: &str = "032615015535634423";

#[derive(Debug, Clone, Copy)]
enum AgentRespState {
    Wait,
    Start,
    Reasoning,
    Messaging,
    Final,
}
