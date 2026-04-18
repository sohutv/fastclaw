use crate::agent::{Agent, AgentResponse};
use crate::channels::{ChannelContext, ChannelMessage, SessionId};
use anyhow::anyhow;
use clap::Parser;
use derive_more::FromStr;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use strum::Display;
use tokio::sync::mpsc::Receiver;

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
        session_id: &SessionId,
    ) -> crate::Result<Receiver<ChannelMessage>> {
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
                    let (tx, rx) = tokio::sync::mpsc::channel(8);
                    let agent = Arc::clone(&agent);
                    let session_id = session_id.clone();
                    let _ = tokio::spawn(async move {
                        let _ = tx
                            .send(ChannelMessage {
                                session_id: session_id.clone(),
                                message: AgentResponse::Notify(
                                    "正在执行会话压缩...".to_string().into(),
                                ),
                            })
                            .await;
                        let result = agent.session_compact(tx.clone(), &session_id, ratio).await;
                        let _ = tx
                            .send(ChannelMessage {
                                session_id,
                                message: AgentResponse::HistoryCompact(result),
                            })
                            .await;
                    });
                    Ok(rx)
                }
            },
            Err(err) => Err(anyhow!(err)),
        }
    }
}
