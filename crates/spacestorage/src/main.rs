mod admin_client;
mod args;
mod exit;
mod http_client;
mod output;

use clap::Parser;
use spacestorage_config::{ValidateOptions, load_file};
use std::process::ExitCode;

const _: () = assert!(
    spacestorage_config::TENANCY_QUOTAS,
    "spacestorage validate must accept quota blocks the daemon accepts"
);

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
        args::Command::Job { action, endpoint } => {
            let endpoint = endpoint.unwrap_or_else(|| "http://127.0.0.1:8080".into());
            let token = std::env::var("SPACESTORAGE_TOKEN").ok();
            job_cli(&endpoint, token.as_deref(), action)
        }
        args::Command::Migrate {
            from,
            to_node,
            to_namespace,
            name,
            policy,
            strategy,
            endpoint,
        } => {
            let endpoint = endpoint.unwrap_or_else(|| "http://127.0.0.1:8080".into());
            let token = std::env::var("SPACESTORAGE_TOKEN").ok();
            let (ns, container) = split_ns_container(&from);
            let mut body = serde_json::json!({
                "source": {"namespace": ns, "container": container},
                "policy": policy,
                "strategy": strategy,
            });
            if let Some(n) = to_node {
                body["dest_nodes"] = serde_json::json!([n]);
            }
            if let Some(n) = to_namespace {
                body["dest_namespace"] = serde_json::json!(n);
            }
            if let Some(n) = name {
                body["dest_name"] = serde_json::json!(n);
            }
            post_job(&endpoint, token.as_deref(), "/v1/migrate", &body)
        }
        args::Command::Transform {
            from,
            rewrite,
            to_type,
            mapping_file,
            swap,
            no_swap,
            retain_source,
            endpoint,
        } => {
            let endpoint = endpoint.unwrap_or_else(|| "http://127.0.0.1:8080".into());
            let token = std::env::var("SPACESTORAGE_TOKEN").ok();
            let (ns, container) = split_ns_container(&from);
            let mut body = serde_json::json!({
                "source": {"namespace": ns, "container": container},
                "rewrite": rewrite,
                "swap": swap && !no_swap,
                "retain_source": retain_source,
                "source_type": "kv_store",
            });
            if let Some(t) = to_type {
                body["target_type"] = serde_json::json!(t);
            }
            if let Some(path) = mapping_file {
                match std::fs::read_to_string(&path) {
                    Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(q) => body["mapping_query"] = q,
                        Err(e) => {
                            eprintln!("mapping-file: {e}");
                            return ExitCode::from(exit::CONFIG_INVALID);
                        }
                    },
                    Err(e) => {
                        eprintln!("mapping-file: {e}");
                        return ExitCode::from(exit::CONFIG_INVALID);
                    }
                }
            }
            post_job(&endpoint, token.as_deref(), "/v1/transform", &body)
        }
        args::Command::Backup {
            namespace,
            container,
            pitr,
            endpoint,
        } => {
            let endpoint = endpoint.unwrap_or_else(|| "http://127.0.0.1:8080".into());
            let token = std::env::var("SPACESTORAGE_TOKEN").ok();
            let scope = if let Some(c) = container {
                let (ns, name) = split_ns_container(&c);
                serde_json::json!({"namespace": ns, "container": name})
            } else if let Some(ns) = namespace {
                serde_json::json!({"name": ns})
            } else {
                eprintln!("backup: require --namespace or --container");
                return ExitCode::from(exit::USAGE);
            };
            let body = serde_json::json!({ "scope": scope, "pitr": pitr });
            post_job(&endpoint, token.as_deref(), "/v1/backup", &body)
        }
        args::Command::Restore {
            snapshot,
            pitr: _,
            namespace,
            key_ref,
            endpoint,
        } => {
            let endpoint = endpoint.unwrap_or_else(|| "http://127.0.0.1:8080".into());
            let token = std::env::var("SPACESTORAGE_TOKEN").ok();
            let mut body = serde_json::json!({
                "snapshot_id": snapshot,
                "confirm_drop": true,
            });
            if let Some(ns) = namespace {
                body["dest_namespace"] = serde_json::json!(ns);
            }
            if let Some(k) = key_ref {
                body["key_ref"] = serde_json::json!(k);
            }
            post_job(&endpoint, token.as_deref(), "/v1/restore", &body)
        }
        args::Command::QueryStats { .. } => {
            eprintln!("query-stats: see /metrics query-processing series (008)");
            ExitCode::from(exit::USAGE)
        }
        args::Command::Ui { endpoint } => {
            print!("{}", spacestorage_admin_ui::ui_cli_hint(&endpoint));
            ExitCode::from(exit::OK)
        }
        args::Command::Ingest {
            action,
            endpoint,
            token,
        } => {
            let token = token.or_else(|| std::env::var("SPACESTORAGE_TOKEN").ok());
            match action {
                args::IngestAction::Syslog => {
                    println!(
                        "syslog bind is node entrypoint config (handler syslog + ingest {{}}); see docs/examples/ingest/ingest-syslog.conf"
                    );
                    ExitCode::from(exit::OK)
                }
                args::IngestAction::Kafka { action } => {
                    ingest_kafka(&endpoint, token.as_deref(), action)
                }
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
        args::Command::Controllers { endpoint, voters } => match voters {
            None => {
                println!("POST {endpoint}/v1/admin op=controllers");
                ExitCode::from(exit::OK)
            }
            Some(args::ControllersVoters::Replace { group, from, to }) => {
                println!(
                    "POST {endpoint}/v1/admin op=voter_replace group={group} from={from} to={to}"
                );
                ExitCode::from(exit::OK)
            }
            Some(args::ControllersVoters::Grow { group, add }) => {
                println!("POST {endpoint}/v1/admin op=voter_grow group={group} add={add:?}");
                ExitCode::from(exit::OK)
            }
            Some(args::ControllersVoters::Shrink { group, remove }) => {
                println!(
                    "POST {endpoint}/v1/admin op=voter_shrink group={group} remove={remove:?}"
                );
                ExitCode::from(exit::OK)
            }
        },
        args::Command::RaftStatus { group, endpoint } => {
            println!("POST {endpoint}/v1/admin op=raft_status group={group}");
            ExitCode::from(exit::OK)
        }
        args::Command::Namespaces { endpoint, output } => admin_client::run_admin_op(
            &endpoint,
            spacestorage_admin_proto::AdminOp::NamespaceList,
            Some(&output),
        ),
        args::Command::Namespace { action, endpoint } => {
            let (op, output) = match action {
                args::NamespaceAction::Create { name } => (
                    spacestorage_admin_proto::AdminOp::NamespaceCreate { name },
                    None,
                ),
                args::NamespaceAction::Rename { name, new_name } => (
                    spacestorage_admin_proto::AdminOp::NamespaceRename { name, new_name },
                    None,
                ),
                args::NamespaceAction::Delete { name, cascade } => (
                    spacestorage_admin_proto::AdminOp::NamespaceDelete { name, cascade },
                    None,
                ),
                args::NamespaceAction::Describe { name, output } => (
                    spacestorage_admin_proto::AdminOp::NamespaceDescribe { name },
                    Some(output),
                ),
            };
            admin_client::run_admin_op(&endpoint, op, output.as_deref())
        }
        args::Command::Quotas { action, endpoint } => {
            let args::QuotasAction::Set {
                name,
                bytes,
                objects,
                connections,
                ops_per_sec,
            } = action;
            let mut quotas = Vec::new();
            for (unit, limit) in [
                ("bytes", bytes),
                ("objects", objects),
                ("connections", connections),
                ("ops_per_sec", ops_per_sec),
            ] {
                if let Some(limit) = limit {
                    quotas.push(spacestorage_admin_proto::QuotaInput {
                        unit: unit.into(),
                        limit,
                        datatype: None,
                    });
                }
            }
            admin_client::run_admin_op(
                &endpoint,
                spacestorage_admin_proto::AdminOp::QuotaReplace { name, quotas },
                None,
            )
        }
        args::Command::Roles { endpoint } => {
            admin_client::run_admin_op(&endpoint, spacestorage_admin_proto::AdminOp::RoleList, None)
        }
    }
}

fn split_ns_container(s: &str) -> (String, String) {
    match s.split_once('.') {
        Some((ns, c)) => (ns.to_string(), c.to_string()),
        None => ("default".into(), s.to_string()),
    }
}

fn print_http_job(r: http_client::HttpResponse) -> ExitCode {
    match r.status {
        200 | 202 => {
            print!("{}", r.body);
            if !r.body.ends_with('\n') {
                println!();
            }
            ExitCode::from(exit::OK)
        }
        401 | 403 => {
            eprintln!("{}", r.body);
            ExitCode::from(exit::UNAUTHORIZED)
        }
        422 | 409 | 404 => {
            eprintln!("{}", r.body);
            ExitCode::from(exit::CONFIG_INVALID)
        }
        501 => {
            eprintln!("{}", r.body);
            ExitCode::from(exit::SLICE10_REQUIRED)
        }
        other => {
            eprintln!("HTTP {other}: {}", r.body);
            ExitCode::from(exit::CONNECT)
        }
    }
}

fn post_job(endpoint: &str, token: Option<&str>, path: &str, body: &serde_json::Value) -> ExitCode {
    let payload = body.to_string();
    match http_client::request("POST", endpoint, path, token, Some(&payload)) {
        Ok(r) => print_http_job(r),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(exit::CONNECT)
        }
    }
}

fn job_cli(endpoint: &str, token: Option<&str>, action: args::JobAction) -> ExitCode {
    match action {
        args::JobAction::List => {
            match http_client::request("GET", endpoint, "/v1/jobs", token, None) {
                Ok(r) => print_http_job(r),
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(exit::CONNECT)
                }
            }
        }
        args::JobAction::Status { id } => {
            match http_client::request("GET", endpoint, &format!("/v1/jobs/{id}"), token, None) {
                Ok(r) => print_http_job(r),
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(exit::CONNECT)
                }
            }
        }
        args::JobAction::Cancel { id } => {
            match http_client::request(
                "POST",
                endpoint,
                &format!("/v1/jobs/{id}/cancel"),
                token,
                None,
            ) {
                Ok(r) => print_http_job(r),
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(exit::CONNECT)
                }
            }
        }
        args::JobAction::Resume { id } => {
            match http_client::request(
                "POST",
                endpoint,
                &format!("/v1/jobs/{id}/resume"),
                token,
                None,
            ) {
                Ok(r) => print_http_job(r),
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(exit::CONNECT)
                }
            }
        }
    }
}

fn ingest_kafka(endpoint: &str, token: Option<&str>, action: args::IngestKafkaAction) -> ExitCode {
    match action {
        args::IngestKafkaAction::List => {
            match http_client::request("GET", endpoint, "/v1/ingest/kafka", token, None) {
                Ok(r) if r.status == 200 => {
                    print!("{}", r.body);
                    if !r.body.ends_with('\n') {
                        println!();
                    }
                    ExitCode::from(exit::OK)
                }
                Ok(r) if r.status == 403 => {
                    eprintln!("{}", r.body);
                    ExitCode::from(exit::UNAUTHORIZED)
                }
                Ok(r) if r.status == 401 => {
                    eprintln!("{}", r.body);
                    ExitCode::from(exit::UNAUTHORIZED)
                }
                Ok(r) => {
                    eprintln!("HTTP {}: {}", r.status, r.body);
                    ExitCode::from(exit::CONNECT)
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(exit::CONNECT)
                }
            }
        }
        args::IngestKafkaAction::Add {
            namespace,
            container,
            brokers,
            topic,
            group,
            format,
            name,
        } => {
            let name = name.unwrap_or_else(|| format!("{namespace}-{container}"));
            let brokers: Vec<String> = brokers
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let body = serde_json::json!({
                "id": "00000000-0000-0000-0000-000000000000",
                "name": name,
                "namespace": namespace,
                "container": container,
                "type": "log_stream",
                "brokers": brokers,
                "topic": topic,
                "group": group,
                "format": format,
                "plaintext": true,
            });
            match http_client::request(
                "POST",
                endpoint,
                "/v1/ingest/kafka",
                token,
                Some(&body.to_string()),
            ) {
                Ok(r) if (200..300).contains(&r.status) => {
                    println!("{}", r.body);
                    ExitCode::from(exit::OK)
                }
                Ok(r) if r.status == 403 => {
                    eprintln!("{}", r.body);
                    ExitCode::from(exit::UNAUTHORIZED)
                }
                Ok(r) => {
                    eprintln!("HTTP {}: {}", r.status, r.body);
                    ExitCode::from(if r.status == 401 {
                        exit::UNAUTHORIZED
                    } else {
                        exit::USAGE
                    })
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(exit::CONNECT)
                }
            }
        }
        args::IngestKafkaAction::Delete { id } => {
            match http_client::request(
                "DELETE",
                endpoint,
                &format!("/v1/ingest/kafka/{id}"),
                token,
                None,
            ) {
                Ok(r) if (200..300).contains(&r.status) || r.status == 204 => {
                    ExitCode::from(exit::OK)
                }
                Ok(r) if r.status == 403 => {
                    eprintln!("{}", r.body);
                    ExitCode::from(exit::UNAUTHORIZED)
                }
                Ok(r) => {
                    eprintln!("HTTP {}: {}", r.status, r.body);
                    ExitCode::from(if r.status == 401 {
                        exit::UNAUTHORIZED
                    } else {
                        exit::USAGE
                    })
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(exit::CONNECT)
                }
            }
        }
    }
}
