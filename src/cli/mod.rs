use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod onboard;
mod start;
#[derive(Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

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
    Onboard(onboard::Onboard),
    Start(start::Start),
}

impl CmdRunner for Command {
    async fn run(&self) -> crate::Result<()> {
        match self {
            Self::Onboard(onboard) => onboard.run().await,
            Self::Start(start) => start.run().await,
        }
    }
}

fn default_config_path() -> PathBuf {
    let user_dirs = directories::UserDirs::new().expect("user home not exist!!!");
    user_dirs.home_dir().join(".fastclaw")
}
