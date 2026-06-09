use clap::Args;

#[derive(Args)]
pub struct Onboard {
    #[arg(long)]
    workdir: Option<std::path::PathBuf>,
}
