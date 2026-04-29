use clap::{Parser, Subcommand};

mod onboard;
mod start;
#[derive(Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[allow(async_fn_in_trait)]
pub trait CmdRunner {
    async fn run(&self) -> crate::Result<()>;
}

impl CmdRunner for Cli {
    async fn run(&self) -> crate::Result<()> {
        self.command.run().await
    }
}

#[derive(Subcommand)]
pub enum Command {
    Start(start::Start),
    Onboard(onboard::Onboard),
}

impl CmdRunner for Command {
    async fn run(&self) -> crate::Result<()> {
        match self {
            Self::Start(start) => start.run().await,
            Self::Onboard(onboard) => onboard.run().await,
        }
    }
}
