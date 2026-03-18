use clap::Parser;
use fastclaw::Result;
use fastclaw::cli::{Cli, CmdRunner};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    logger_init().await?;
    let _ = cli.run().await?;
    Ok(())
}

async fn logger_init() -> Result<()> {
    env_logger::init();
    Ok(())
}
