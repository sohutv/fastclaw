use crate::agent::{AgentMessageSender, AgentSignal};
use crate::channels::console_cmd::Console;
use crate::channels::{ChannelContext, ChannelMessageSender};
use crate::config::{Config, DingTalkConfig};
use anyhow::anyhow;
use async_trait::async_trait;
use dingtalk_stream::frames::{CallbackMessageData, CallbackMessagePayload, RichTextItem};
use dingtalk_stream::{CallbackMessage, DingTalkStream, Error, ErrorCode, MessageTopic, Resp};
use itertools::Itertools;
use rig::completion::{AssistantContent, Message};
use rig::message::ReasoningContent;
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

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
    async fn process(&self, CallbackMessage { data, .. }: &CallbackMessage) -> Result<Resp, Error> {
        let Some(CallbackMessageData {
            msg_id: Some(_),
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
        let ctx = Arc::clone(&ctx);
        let join_handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("unexpected err");
            let ctx0 = Arc::clone(&ctx);
            let dingtalk_stream_handle = rt.spawn(async move {
                let cb_handler = DingTalkCallbackHandler {
                    ctx: ctx0,
                    dingtalk_config: dingtalk_config.clone(),
                    dingtalk_bot_topic: MessageTopic::Callback(
                        dingtalk_stream::TOPIC_ROBOT.to_string(),
                    ),
                    agent_message_sender,
                };
                let mut dingtalk_stream = DingTalkStream::new(dingtalk_config.credential)
                    .register_callback_handler(cb_handler);
                dingtalk_stream.start_forever().await;
            });
            let ctx1 = Arc::clone(&ctx);
            let agent_handle = rt.spawn(async move {
                let ctx = Arc::clone(&ctx1);
                let mut receiver = receiver;
                let _ = DingtalkChannel::poll_agent_signal(&ctx, &mut receiver).await;
            });
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
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Init;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tokio::select! {
                message = receiver.recv() => {
                    if let Some(signal) = message {
                        let ctx = ctx.read().await;
                        match  Self::handle_agent_signal(&*ctx, &signal, state).await{
                            Ok(AgentRespState::Final) | Err( _)=> {
                                // return Ok(());
                            },
                            Ok(next)=>{
                                state = next;
                            }
                        }
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
                            print!("{}", text);
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
