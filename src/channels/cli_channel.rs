use crate::agent::{Agent, AgentRequest, AgentResponse, Notify};
use crate::channels::console_cmd::Console;
use crate::channels::{Channel, ChannelContext, ChannelMessage, SessionId};
use crate::config::{Config, Workspace};
use anyhow::anyhow;
use async_trait::async_trait;
use rig::completion::Message;
use rig::message::{AssistantContent, ReasoningContent, ToolCall, ToolFunction};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::{Write, stdout};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc::Receiver;

pub struct CliChannel {
    ctx: Arc<ChannelContext>,
    session_id: SessionId,
}

impl CliChannel {
    pub fn new(config: &'static Config, workspace: &'static Workspace) -> crate::Result<Self> {
        Ok(CliChannel {
            ctx: Arc::new(ChannelContext {
                config: config.clone(),
                workspace,
            }),
            session_id: SessionId::Master {
                val: "cli-session-channel".into(),
                settings: Default::default(),
            },
        })
    }
}

#[async_trait]
impl Channel for CliChannel {
    type Output = JoinHandle<()>;

    async fn start(self, agent: Arc<dyn Agent>) -> crate::Result<Self::Output> {
        let Self { ctx, session_id } = self;
        let ctx = Arc::clone(&ctx);
        let (message_sender, mut message_receiver) = tokio::sync::mpsc::channel(32);
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
                                    match Console::handle_console_cmd(
                                        &ctx,
                                        &line,
                                        &agent,
                                        &session_id,
                                    )
                                    .await
                                    {
                                        Ok(_) => {
                                            continue;
                                        }
                                        Err(_) => {}
                                    }
                                }
                                let message = Message::user(line);
                                let _ = agent
                                    .run(
                                        AgentRequest {
                                            id: Default::default(),
                                            session_id: session_id.clone(),
                                            message,
                                        },
                                        message_sender.clone(),
                                    )
                                    .await;
                                let _ = Self::poll_agent_message(&ctx, &mut message_receiver).await;
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
}

impl CliChannel {
    async fn poll_agent_message(
        ctx: &Arc<ChannelContext>,
        receiver: &mut Receiver<ChannelMessage>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Init;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tokio::select! {
                message = receiver.recv() => {
                    if let Some(message) = message {
                        match  Self::handle_agent_message(&ctx, &message, state).await{
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

    async fn handle_agent_message(
        ctx: &ChannelContext,
        agent_response: &AgentResponse,
        curr_state: AgentRespState,
    ) -> crate::Result<AgentRespState> {
        match agent_response {
            AgentResponse::Start => {
                if let AgentRespState::Init = curr_state {
                    Ok(AgentRespState::Start)
                } else {
                    Err(anyhow!("AgentRespState must be Init when starting"))
                }
            }
            AgentResponse::ToolCall(ToolCall {
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
            AgentResponse::ReasoningStream(reasoning) => {
                match curr_state {
                    AgentRespState::Start => {
                        cli_line_clear();
                        if ctx.config.default_show_reasoning {
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
            AgentResponse::MessageStream(message) => {
                match curr_state {
                    AgentRespState::Start => {
                        cli_line_clear();
                    }
                    AgentRespState::Reasoning => {
                        if ctx.config.default_show_reasoning {
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
            AgentResponse::Final(usage) => {
                println!(
                    r#"
<<Tokens:{}↑{}↓{}>>
                    "#,
                    usage.total_tokens, usage.input_tokens, usage.output_tokens
                );
                Ok(AgentRespState::Final)
            }
            AgentResponse::Error(error) => {
                cli_line_clear();
                eprintln!("{}", error);
                Err(anyhow!("Agent error: {}", error))
            }
            AgentResponse::HistoryCompact { .. } => Ok(curr_state),
            AgentResponse::Notify(notify) => {
                match notify {
                    Notify::Text(text) => {
                        println!(
                            r#"
Notify >> ////////
{}
//////// << Notify
                "#,
                            text
                        );
                    }
                    Notify::Markdown { title, content } => {
                        println!(
                            r#"
Notify >> ////////
Title: {}
{}
//////// << Notify
                "#,
                            title, content
                        );
                    }
                }
                Ok(curr_state)
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
