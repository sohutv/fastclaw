use crate::agent::{AgentMessageSender, AgentSignal};
use crate::channels::console_cmd::Console;
use crate::channels::{ChannelContext, ChannelMessageSender};
use crate::config::{Config, DingTalkConfig};
use anyhow::anyhow;
use async_trait::async_trait;
use dingtalk_stream::client::DingtalkMessageSender;
use dingtalk_stream::frames::{
    CallbackMessageData, CallbackMessagePayload, CallbackWebhookMessage, RichTextItem,
    RobotBatchMessage,
};
use dingtalk_stream::{CallbackMessage, DingTalkStream, Error, ErrorCode, MessageTopic, Resp};
use itertools::Itertools;
use rig::completion::{AssistantContent, Message};
use rig::message::ReasoningContent;
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
        let mut state = AgentRespState::Init;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tokio::select! {
                message = receiver.recv() => {
                    if let Some(signal) = message {
                        let ctx = ctx.read().await;
                        match  Self::handle_agent_signal(&*ctx, &signal, state, &dingtalk_msg_sender).await{
                            Ok(AgentRespState::Final) | Err( _)=> {
                                // return Ok(());
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
                        AgentRespState::Init|AgentRespState::Start => {
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
        dingtalk_msg_sender: &DingtalkMessageSender,
    ) -> crate::Result<AgentRespState> {
        match signal {
            AgentSignal::Start => {
                if let AgentRespState::Init = curr_state {
                    Ok(AgentRespState::Start)
                } else {
                    Err(anyhow!("AgentRespState must be Init when starting"))
                }
            }
            AgentSignal::ReasoningStream(reasoning) => {
                match curr_state {
                    AgentRespState::Start => {
                        if ctx.config.show_reasoning {
                            //todo!("Reasoning start")
                        }
                    }
                    _ => {}
                }
                for content in reasoning.content.iter() {
                    if let ReasoningContent::Text { text, .. } = content {
                        if !text.is_empty() {
                            let _ = dingtalk_msg_sender
                                .send(RobotBatchMessage {
                                    user_ids: vec!["032615015535634423".into()],
                                    content: text.into(),
                                    send_result_cb: None,
                                })
                                .await;
                        }
                    }
                }
                Ok(AgentRespState::Reasoning)
            }
            AgentSignal::MessageStream(message) => {
                match curr_state {
                    AgentRespState::Start => {
                        //todo!()
                    }
                    AgentRespState::Reasoning => {
                        if ctx.config.show_reasoning {
                            //todo!("Reasoning end")
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
                                        print!("{}", text);
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
                println!(
                    r#"
<<Tokens:{}↑{}↓{}>>
                    "#,
                    usage.total_tokens, usage.input_tokens, usage.output_tokens
                );
                Ok(AgentRespState::Final)
            }
            AgentSignal::Error(error) => {
                eprintln!("{}", error);
                Err(anyhow!("Agent error: {}", error))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AgentRespState {
    Init,
    Start,
    Reasoning,
    Messaging,
    Final,
}
