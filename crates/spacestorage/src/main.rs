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
        args::Command::Executions { .. } => {
            eprintln!("executions: use GET /v1/executions on admin (005)");
            ExitCode::from(exit::USAGE)
        }
        args::Command::Explain { sql, .. } => {
            eprintln!("explain: POST /v1/explain — local: {sql}");
            ExitCode::from(exit::USAGE)
        }
        args::Command::Jobs { .. } => {
            eprintln!("jobs: GET /v1/jobs | wait | cancel (005 slice 8)");
            ExitCode::from(exit::USAGE)
        }
        args::Command::Job { action, .. } => {
            let svc = spacestorage_migrate::stub_service();
            if !svc.slice10_enabled {
                eprintln!("MigrateSlice10Required: data_* jobs require slice 10");
                return ExitCode::from(exit::SLICE10_REQUIRED);
            }
            match action {
                args::JobAction::List => {
                    println!("{{\"jobs\":[]}}");
                    ExitCode::from(exit::OK)
                }
                args::JobAction::Status { id }
                | args::JobAction::Cancel { id }
                | args::JobAction::Resume { id } => {
                    eprintln!("job {id}: use admin-http /v1/jobs");
                    ExitCode::from(exit::USAGE)
                }
            }
        }
        args::Command::Migrate { .. }
        | args::Command::Transform { .. }
        | args::Command::Backup { .. }
        | args::Command::Restore { .. } => {
            if cfg!(feature = "migration-backup") {
                eprintln!("use POST /v1/migrate|/v1/transform|/v1/backup|/v1/restore");
                ExitCode::from(exit::USAGE)
            } else {
                eprintln!(
                    "MigrateSlice10Required: enable migration-backup profile / jobs.enabled"
                );
                ExitCode::from(exit::SLICE10_REQUIRED)
            }
        }
        args::Command::QueryStats { .. } => {
            eprintln!("query-stats: see /metrics query-processing series (008)");
            ExitCode::from(exit::USAGE)
        }
        args::Command::Ui { endpoint } => {
            if cfg!(feature = "complete-product") {
                print!("{}", spacestorage_admin_ui::ui_cli_hint(&endpoint));
                ExitCode::from(exit::OK)
            } else {
                eprintln!("UiIngestSlice11Required: enable complete-product profile");
                ExitCode::from(exit::CONFIG_INVALID)
            }
        }
        args::Command::Ingest { action } => {
            if !cfg!(feature = "complete-product") {
                eprintln!("UiIngestSlice11Required: enable complete-product profile");
                return ExitCode::from(exit::CONFIG_INVALID);
            }
            match action {
                args::IngestAction::Syslog => {
                    println!(
                        "syslog bind is node entrypoint config (handler syslog + ingest {{}}); see docs/examples/ingest/ingest-syslog.conf"
                    );
                    ExitCode::from(exit::OK)
                }
                args::IngestAction::Kafka { action } => match action {
                    args::IngestKafkaAction::List => {
                        println!("{{\"kafka\":[]}}");
                        ExitCode::from(exit::OK)
                    }
                    args::IngestKafkaAction::Add { .. } => {
                        eprintln!("use POST /v1/ingest/kafka");
                        ExitCode::from(exit::USAGE)
                    }
                    args::IngestKafkaAction::Delete { id } => {
                        eprintln!("use DELETE /v1/ingest/kafka/{id}");
                        ExitCode::from(exit::USAGE)
                    }
                },
            }
        }
        args::Command::L0 { action, endpoint } => {
            match action {
                args::L0Action::List => {
                    println!("GET {endpoint}/v1/l0/containers");
                }
                args::L0Action::Create {
                    namespace,
                    name,
                    type_name,
                } => {
                    println!(
                        "POST {endpoint}/v1/l0/containers body={{\"namespace\":\"{namespace}\",\"name\":\"{name}\",\"type_name\":\"{type_name}\"}}"
                    );
                }
                args::L0Action::Get { namespace, name } => {
                    println!("GET {endpoint}/v1/l0/containers/{namespace}/{name}");
                }
            }
            ExitCode::from(exit::OK)
        }
        args::Command::Legal { action, endpoint } => {
            match action {
                args::LegalAction::Hold { .. } => {
                    println!("POST {endpoint}/v1/legal/holds");
                }
                args::LegalAction::Erase { .. } => {
                    println!("POST {endpoint}/v1/legal/erase");
                }
            }
            ExitCode::from(exit::OK)
        }
        args::Command::Cdc { endpoint } => {
            println!("POST {endpoint}/v1/cdc/streams");
            ExitCode::from(exit::OK)
        }
        args::Command::Composition { endpoint } => {
            println!("POST {endpoint}/v1/compositions");
            ExitCode::from(exit::OK)
        }
        args::Command::Billing { endpoint } => {
            println!("POST {endpoint}/v1/billing/estimate");
            ExitCode::from(exit::OK)
        }
        args::Command::Kms { endpoint } => {
            println!("GET {endpoint}/v1/kms/status");
            ExitCode::from(exit::OK)
        }
    }
}
