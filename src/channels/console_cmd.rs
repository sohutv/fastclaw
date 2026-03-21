use crate::channels::ChannelContext;
use clap::Parser;
use derive_more::FromStr;
use std::sync::Arc;
use strum::Display;
use tokio::sync::RwLock;

#[derive(Debug, clap::Parser)]
#[command(name = "/")]
pub enum Console {
    #[command(name = "showreasoning")]
    ShowReasoning {
        /// 状态: on 或 off
        state: ShowReasoning,
    },
    /// 开启紧凑模式: /compact
    #[command(name = "compact")]
    Compact,
}

#[derive(Debug, Clone, Copy, FromStr, Display)]
pub enum ShowReasoning {
    On,
    Off,
}

impl Console {
    pub async fn handle_console_cmd(ctx: Arc<RwLock<ChannelContext>>, line: &str) {
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
                    todo!()
                }
            },
            Err(err) => {
                eprintln!("Error: {}", err.to_string());
            }
        }
    }
}
