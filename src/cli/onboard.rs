use crate::cli::CmdRunner;
use crate::config::Config;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use derive_more::FromStr;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct Onboard {
    #[command(subcommand)]
    command: Command,
}

impl CmdRunner for Onboard {
    async fn run(&self) -> crate::Result<()> {
        self.command.run().await
    }
}

#[derive(Subcommand)]
pub enum Command {
    InitConfig(InitConfig),
}

impl CmdRunner for Command {
    async fn run(&self) -> crate::Result<()> {
        match self {
            Self::InitConfig(init_config) => init_config.run().await,
        }
    }
}
#[derive(Args)]
pub struct InitConfig {
    #[arg(long)]
    path: Option<PathBuf>,
    #[arg(long, default_value = "false")]
    rewrite: bool,
}

impl CmdRunner for InitConfig {
    async fn run(&self) -> crate::Result<()> {
        let Self { path, rewrite } = self;
        let default_path = super::default_config_path();
        let config_dir = path.as_deref().unwrap_or_else(|| &default_path);
        if config_dir.exists() {
            if config_dir.is_file() {
                return Err(anyhow!(
                    "Unexpected file at path: {}, expect directory but got file",
                    config_dir.display()
                ));
            }
            if *rewrite {
                tokio::fs::remove_dir_all(config_dir).await?;
            } else {
                return Err(anyhow!(
                    "Config file already exists at {}",
                    config_dir.display()
                ));
            }
        }
        if config_dir.exists() {
            return Err(anyhow!(
                "Config directory already exists at {}",
                config_dir.display()
            ));
        }
        tokio::fs::create_dir_all(config_dir).await?;
        init_config_file(config_dir).await?;
        init_config_workspace(config_dir).await?;
        log::info!("Fastclaw Config initialized at {}", config_dir.display());
        Ok(())
    }
}

async fn init_config_file(config_dir: &Path) -> crate::Result<()> {
    let config = Config::default();
    let config_path = config_dir.join("config.toml");
    tokio::fs::write(&config_path, toml::to_string_pretty(&config)?).await?;
    Ok(())
}

async fn init_config_workspace(config_dir: &Path) -> crate::Result<()> {
    let workspace = config_dir.join("workspace");
    tokio::fs::create_dir_all(&workspace).await?;

    let cron = workspace.join("cron");
    tokio::fs::create_dir_all(&cron).await?;
    tokio::fs::write(&cron.join("README.md"), "#Cron").await?;

    let memory = workspace.join("memory");
    tokio::fs::create_dir_all(&memory).await?;
    tokio::fs::write(memory.join("README.md"), "#Memories").await?;

    let sessions = workspace.join("sessions");
    tokio::fs::create_dir_all(&sessions).await?;
    tokio::fs::write(sessions.join("README.md"), "#Sessions").await?;

    let skills = workspace.join("skills");
    tokio::fs::create_dir_all(&skills).await?;
    tokio::fs::write(skills.join("README.md"), "#Skills").await?;

    let state = workspace.join("state");
    tokio::fs::create_dir_all(&state).await?;
    tokio::fs::write(state.join("README.md"), "#State").await?;

    tokio::fs::write(workspace.join("AGENTS.md"), include_str!("../../resources/AGENTS.md")).await?;
    tokio::fs::write(workspace.join("BOOTSTRAP.md"), include_str!("../../resources/BOOTSTRAP.md")).await?;
    tokio::fs::write(workspace.join("HEARTBEAT.md"), include_str!("../../resources/HEARTBEAT.md")).await?;
    tokio::fs::write(workspace.join("IDENTITY.md"), include_str!("../../resources/IDENTITY.md")).await?;
    tokio::fs::write(workspace.join("MEMORY.md"), include_str!("../../resources/MEMORY.md")).await?;
    tokio::fs::write(workspace.join("SOUL.md"), include_str!("../../resources/SOUL.md")).await?;
    tokio::fs::write(workspace.join("TOOLS.md"), include_str!("../../resources/TOOLS.md")).await?;
    tokio::fs::write(workspace.join("USER.md"), include_str!("../../resources/USER.md")).await?;

    Ok(())
}
