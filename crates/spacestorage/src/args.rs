use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "spacestorage", about = "SpaceStorage admin CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Validate {
        config: PathBuf,
        #[arg(long = "set")]
        set: Vec<String>,
    },
    Status {
        #[arg(long)]
        endpoint: Option<String>,
    },
}
