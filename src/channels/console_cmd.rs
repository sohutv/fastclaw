use crate::agent::{Agent, AgentResponse};
use crate::channels::{ChannelContext, ChannelMessage, SessionId};
use clap::Parser;
use derive_more::FromStr;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use strum::Display;
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
    Compact {
        #[arg(long, default_value_t = 0.8)]
        ratio: f32,
    },
}

#[derive(Debug, Clone, Copy, FromStr, Display, Serialize, Deserialize)]
pub enum ShowReasoning {
    On,
    Off,
}

impl Console {
    pub async fn handle_console_cmd(
        _: &ChannelContext,
        line: &str,
        agent: &Arc<dyn Agent>,
        channel_message_sender: Sender<ChannelMessage>,
        session_id: &SessionId,
    ) {
        let line = format!("/ {}", &line[1..]);
        match Console::try_parse_from(line.split(" ")) {
            Ok(command) => match command {
                Console::ShowReasoning { state } => match state {
                    ShowReasoning::On => {
                        unimplemented!()
                    }
                    ShowReasoning::Off => {
                        unimplemented!()
                    }
                },
                Console::Compact { ratio } => {
                    let _ = channel_message_sender
                        .send(ChannelMessage {
                            session_id: session_id.clone(),
                            message: AgentResponse::Notify(
                                "正在执行会话压缩...".to_string().into(),
                            ),
                        })
                        .await;
                    let result = agent.session_compact(session_id, ratio).await;
                    let _ = channel_message_sender
                        .send(ChannelMessage {
                            session_id: session_id.clone(),
                            message: AgentResponse::HistoryCompact(result),
                        })
                        .await;
                }
            },
            Err(err) => {
                eprintln!("Error: {}", err.to_string());
            }
        }
    }
}
