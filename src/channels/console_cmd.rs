use crate::agent::{Agent, AgentResponse, Notify};
use crate::channels::{ChannelContext, ChannelMessage, SessionId};
use clap::Parser;
use derive_more::FromStr;
use std::sync::Arc;
use strum::Display;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;

#[derive(Debug, clap::Parser)]
#[command(name = "/")]
pub enum Console {
    #[command(name = "showreasoning")]
    ShowReasoning {
        /// 状态: on 或 off
        state: ShowReasoning,
    },
    /// 压缩 session history: /compact
    #[command(name = "compact")]
    Compact,
}

#[derive(Debug, Clone, Copy, FromStr, Display)]
pub enum ShowReasoning {
    On,
    Off,
}

impl Console {
    pub async fn handle_console_cmd(
        ctx: Arc<RwLock<ChannelContext>>,
        line: &str,
        agent: &Box<dyn Agent>,
        channel_message_sender: Sender<ChannelMessage>,
        session_id: &SessionId,
    ) {
        let line = format!("/ {}", &line[1..]);
        match Console::try_parse_from(line.split(" ")) {
            Ok(command) => match command {
                Console::ShowReasoning { state } => {
                    let mut ctx = ctx.write().await;
                    match state {
                        ShowReasoning::On => {
                            ctx.config.show_reasoning = true;
                        }
                        ShowReasoning::Off => {
                            ctx.config.show_reasoning = false;
                        }
                    }
                }
                Console::Compact => {
                    let _ = channel_message_sender
                        .send(ChannelMessage {
                            session_id: session_id.clone(),
                            message: AgentResponse::Notify(Notify {
                                title: "会话压缩".to_string(),
                                content: "开始执行会话历史压缩任务...".to_string(),
                            }),
                        })
                        .await;
                    let _ = agent
                        .session_compact(channel_message_sender, session_id)
                        .await;
                }
            },
            Err(err) => {
                eprintln!("Error: {}", err.to_string());
            }
        }
    }
}
