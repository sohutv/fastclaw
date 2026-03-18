use crate::agent::Agent;
use crate::cli::CmdRunner;
use crate::config::Config;
use anyhow::anyhow;
use clap::Args;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Args)]
pub struct Start {
    #[arg(long)]
    config_path: Option<PathBuf>,
}

impl CmdRunner for Start {
    async fn run(&self) -> crate::Result<()> {
        let Self { config_path } = self;
        let default_path = super::default_config_path();
        let config_path = config_path.as_deref().unwrap_or_else(|| &default_path);
        if !config_path.exists() {
            return Err(anyhow!(
                "Config file does not exist: {}",
                config_path.display()
            ));
        }
        let config_toml = tokio::fs::read_to_string(config_path.join("config.toml")).await?;
        let config = Box::leak(Box::new(toml::from_str::<Config>(&config_toml)?));
        let ctx = Arc::new(crate::agent::Context::new(config, config_path)?);
        let mut main_agent = Agent::new(
            "main",
            Arc::clone(&ctx),
            config.default_model_provider()?,
            config.default_model().clone(),
        )?;
        let main_agent_channel_sender = main_agent.channel_sender();
        let main_agent_handle = tokio::spawn(async move {
            main_agent.run().await;
        });
        main_agent_channel_sender.send("hello".into()).await;
        main_agent_handle.await?;
        Ok(())
    }
}
