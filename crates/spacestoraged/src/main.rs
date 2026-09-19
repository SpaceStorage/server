use clap::Parser;
use spacestorage_config::{load_file, ValidateOptions};
use spacestorage_node::{first_binary_handler_names, runtime, Node};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "spacestoraged", about = "SpaceStorage node daemon (slices-1-5 / first-binary)")]
struct Args {
    #[arg(long)]
    config: PathBuf,
    #[arg(long = "set", value_name = "KEY=VALUE")]
    set: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();
    let args = Args::parse();
    let handlers = first_binary_handler_names();
    let (cfg, _) = load_file(
        &args.config,
        &args.set,
        &handlers,
        ValidateOptions::first_binary(),
    )
    .map_err(|errs| {
        for e in &errs {
            eprintln!("{e}");
        }
        anyhow::anyhow!("config invalid")
    })?;
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let rt = runtime::build_runtime(threads);
    rt.block_on(async move {
        let node = Node::boot_first_binary(args.config.clone(), cfg, threads, source);
        node.run().await.map_err(|e| anyhow::anyhow!(e))
    })
}
