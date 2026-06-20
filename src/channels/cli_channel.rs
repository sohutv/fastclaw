use crate::agent::{
    Agent, AgentRequest, AgentResponse, AgentVisitor, DelegatedAgent, MainAgent, Notify,
};
use crate::channels::console_cmd::Console;
use crate::channels::{
    Channel, ChannelContext, ChannelMessage, ChannelNotifier, SessionId, SessionSettings,
};
use anyhow::anyhow;
use async_trait::async_trait;
use log::warn;
use rig::OneOrMany;
use rig::completion::Message;
use rig::message::{AssistantContent, ReasoningContent, ToolCall, ToolFunction, UserContent};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::{Write, stdout};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc::Receiver;

#[derive(Clone)]
pub struct CliChannel {
    context: &'static ChannelContext,
    session_id: SessionId,
    session_settings: SessionSettings,
    client: (),
    agent: Arc<MainAgent>,
}

impl CliChannel {
    pub async fn new(
        context: &'static ChannelContext,
        agent: &Arc<MainAgent>,
    ) -> crate::Result<Self> {
        let session_id = SessionId::Master("cli-session-channel".into());
        let session_settings = SessionSettings::default();
        Ok(CliChannel {
            context,
            session_id,
            session_settings,
            client: Default::default(),
            agent: Arc::clone(agent),
        })
    }
}

#[async_trait]
impl Channel for CliChannel {
    type Client = ();
    type JoinHandle = JoinHandle<()>;

    async fn start(
        &'static self,
    ) -> crate::Result<(&'static Self, ChannelNotifier, Self::JoinHandle)> {
        let join_handle = {
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
                                            self.context,
                                            &line,
                                            self.agent.delegated(),
                                            &self.session_id,
                                        )
                                        .await
                                        {
                                            Ok(_) => {
                                                continue;
                                            }
                                            Err(_) => {}
                                        }
                                    }
                                    if let Ok(join_handle) = self
                                        .spawn_agent_request(AgentRequest {
                                            id: Default::default(),
                                            session_id: self.session_id.clone(),
                                            agent_id: self.agent.id().clone(),
                                            message: vec![OneOrMany::one(UserContent::text(line))],
                                            addi_preamble: None,
                                        })
                                        .await
                                    {
                                        let _ = join_handle.await;
                                    }
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
            join_handle
        };
        let notifier = {
            let (tx, mut rx) = tokio::sync::mpsc::channel::<super::Notify>(32);
            while let Some(notify) = rx.recv().await {
                println!(
                    r#"
//////// Notify: {}
{}
"#,
                    notify.title, notify.content
                );
            }
            ChannelNotifier::from(tx)
        };
        Ok((self, notifier, join_handle))
    }

    fn context(&self) -> &'static ChannelContext {
        self.context
    }

    async fn client(&self) -> crate::Result<Self::Client> {
        Ok(self.client)
    }

    async fn handle_agent_message(
        &self,
        _: &Self::Client,
        _message_from: Arc<dyn Agent>,
        receiver: &mut Receiver<crate::Result<ChannelMessage>>,
    ) -> crate::Result<()> {
        let mut state = AgentRespState::Init;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tokio::select! {
                message = receiver.recv() => {
                    if let Some(message) = message {
                        match message{
                            Ok(message) =>{
                                match  self.handle_agent_message_actual(&message, state).await{
                                    Ok(AgentRespState::Final) | Err( _)=> {
                                        return Ok(());
                                    },
                                    Ok(next)=>{
                                        state = next;
                                    }
                                }
                            },
                            Err(err)=> {
                                warn!("recv error channel message: {err}");
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

    fn allow_session_ids(&self) -> crate::Result<Vec<&SessionId>> {
        Ok(vec![&self.session_id])
    }
}

impl CliChannel {
    async fn handle_agent_message_actual(
        &self,
        ChannelMessage {
            message: agent_response,
            ..
        }: &ChannelMessage,
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
                        if self.session_settings.show_reasoning {
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
                        if self.session_settings.show_reasoning {
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
