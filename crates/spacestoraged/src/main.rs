use clap::Parser;
use spacestorage_config::{load_file, ValidateOptions};
use spacestorage_node::{first_binary_handler_names, logging, runtime, Node};
use std::path::PathBuf;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, reload, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "spacestoraged", about = "SpaceStorage node daemon (slices-1-5 / first-binary)")]
struct Args {
    #[arg(long)]
    config: PathBuf,
    #[arg(long = "set", value_name = "KEY=VALUE")]
    set: Vec<String>,
}

fn main() -> anyhow::Result<()> {
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

    let filter = EnvFilter::try_new(&cfg.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, reload_handle) = reload::Layer::new(filter);
    let registry = tracing_subscriber::registry().with(filter_layer);
    if cfg.log_format == "json" {
        registry.with(fmt::layer().json()).init();
    } else {
        registry.with(fmt::layer()).init();
    }
    logging::install_reload_handle(reload_handle);

    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let rt = runtime::build_runtime(threads);
    rt.block_on(async move {
        let node = Node::boot_first_binary(args.config.clone(), cfg, threads, source);
        node.run().await.map_err(|e| anyhow::anyhow!(e))
    })
}
