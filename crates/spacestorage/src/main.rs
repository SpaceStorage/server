mod args;
mod exit;
mod output;

use clap::Parser;
use spacestorage_config::{load_file, ValidateOptions};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = args::Cli::parse();
    match args.command {
        args::Command::Validate { config, set } => {
            let handlers = [
                "admin",
                "admin-http",
                "internode",
                "replication",
                "postgresql",
                "redis",
                "echo",
                "cassandra",
            ];
            match load_file(
                &config,
                &set,
                &handlers,
                ValidateOptions::fixtures_offline(),
            ) {
                Ok((cfg, _)) => {
                    let threads = cfg.threads.unwrap_or_else(|| {
                        std::thread::available_parallelism()
                            .map(|n| n.get() as u32)
                            .unwrap_or(1)
                    });
                    println!("ok node={} threads={}", cfg.node_name, threads);
                    ExitCode::from(exit::OK)
                }
                Err(errs) => {
                    for e in errs {
                        eprintln!("{e}");
                    }
                    ExitCode::from(exit::CONFIG_INVALID)
                }
            }
        }
        args::Command::Status { .. } => {
            eprintln!("status: remote admin client not fully wired in this slice");
            ExitCode::from(exit::USAGE)
        }
    }
}
