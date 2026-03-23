use crate::agent::{AgentMessage, AgentMessageSender, AgentSignal};
use crate::channels::console_cmd::Console;
use crate::channels::{ChannelContext, ChannelMessage, ChannelMessageSender, Session, SessionId};
use crate::config::Config;
use crate::hash_map;
use anyhow::anyhow;
use rig::completion::Message;
use rig::message::{AssistantContent, ReasoningContent, ToolCall, ToolFunction};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::{Write, stdout};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

pub struct CliChannel {
    ctx: Arc<RwLock<ChannelContext>>,
    agent_signal_receiver: Receiver<ChannelMessage>,
    agent_message_sender: AgentMessageSender,
}

impl CliChannel {
    pub(super) fn new(
        config: &'static Config,
        agent_message_sender: AgentMessageSender,
    ) -> crate::Result<(Self, ChannelMessageSender)> {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        Ok((
            CliChannel {
                ctx: Arc::new(RwLock::new(ChannelContext {
                    config: config.clone(),
                    sessions: {
                        let session_id = SessionId::from("cli-session-channel".to_string());
                        hash_map!(session_id.clone() => Session::Private{session_id: session_id})
                    },
                })),
                agent_signal_receiver: receiver,
                agent_message_sender,
            },
            sender.into(),
        ))
    }
}

impl CliChannel {
    pub async fn start(self) -> crate::Result<JoinHandle<()>> {
        let Self {
            ctx,
            agent_signal_receiver: mut receiver,
            agent_message_sender,
        } = self;
        let ctx = Arc::clone(&ctx);
        let join_handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("unexpected err");
            let mut rl = DefaultEditor::new().expect("unexpected err");
            let _ = rt.block_on(async move {
                loop {
                    let readline = rl.readline(">> ");
                    match readline {
                        Ok(line) => {
                            let line = line.trim();
                            if !line.is_empty() {
                                if line.starts_with('/') {
                                    Console::handle_console_cmd(Arc::clone(&ctx), &line).await;
                                    continue;
                                }
                                let ctx = ctx.read().await;
                                let session_id =
                                    ctx.sessions.keys().next().expect("unexpected sessions");
                                let message = Message::user(line);
                                let _ = agent_message_sender
                                    .send(AgentMessage::Private {
                                        session_id: session_id.clone(),
                                        message,
                                    })
                                    .await;
                                let _ = Self::poll_agent_signal(&ctx, &mut receiver).await;
                            }
                        }
                        Err(ReadlineError::Interrupted) => {
                            println!("CTRL-C");
                            break;
                        }
                        Err(err) => {
                            eprintln!("Error: {:?}", err);
                        }
                    }
                }
            });
        });
        Ok(join_handle)
    }

    async fn poll_agent_signal(
        ctx: &ChannelContext,
        receiver: &mut Receiver<ChannelMessage>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Init;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tokio::select! {
                message = receiver.recv() => {
                    if let Some(ChannelMessage::Private {signal,..})|Some(ChannelMessage::Group {signal,}) = message {
                        match  Self::handle_agent_signal(ctx, &signal, state).await{
                            Ok(AgentRespState::Final) | Err( _)=> {
                                return Ok(());
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
                           let mut stdout = stdout();
                            print!(".");
                            stdout.flush().expect("unexpected error");
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
            AgentSignal::ToolCall(ToolCall {
                function: ToolFunction { name, arguments },
                ..
            }) => {
                println!(
                    r#"
//////// ToolCall: {name}
{}
"#,
                    serde_json::to_string_pretty(arguments)
                        .unwrap_or_else(|err| format!("Error serializing arguments: {}", err))
                );
                Ok(curr_state)
            }
            AgentSignal::ReasoningStream(reasoning) => {
                match curr_state {
                    AgentRespState::Start => {
                        cli_line_clear();
                        if ctx.config.show_reasoning {
                            println!(
                                r#"
Reasoning >> ////////
"#
                            );
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
                        cli_line_clear();
                    }
                    AgentRespState::Reasoning => {
                        if ctx.config.show_reasoning {
                            println!(
                                r#"
//////// << Reasoning
"#
                            );
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
            AgentSignal::Message(message) => {
                match message {
                    Message::Assistant { content, .. } => {
                        for content in content.iter() {
                            match content {
                                AssistantContent::Text(text) => {
                                    println!("{}", text.to_string());
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                Ok(curr_state)
            }
            AgentSignal::Error(error) => {
                cli_line_clear();
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

fn cli_line_clear() {
    print!("\r\x1b[K");
}
