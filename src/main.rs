use clap::Parser;
use fastclaw::Result;
use fastclaw::cli::{Cli, CmdRunner};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let _ = cli.run().await?;
    Ok(())
}
