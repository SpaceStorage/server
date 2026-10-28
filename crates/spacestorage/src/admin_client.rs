//! Admin HTTP client for tenancy / control-plane CLI ops.

use crate::exit;
use crate::http_client;
use spacestorage_admin_proto::{AdminOp, ErrorBody};
use std::process::ExitCode;

pub fn run_admin_op(
    endpoint: &str,
    op: AdminOp,
    output: Option<&str>,
) -> ExitCode {
    let token = std::env::var("SPACESTORAGE_TOKEN").ok();
    match http_client::request(
        "POST",
        endpoint,
        "/v1/admin",
        token.as_deref(),
        Some(&serde_json::to_string(&op).unwrap_or_else(|_| "{}".into())),
    ) {
        Ok(r) if r.status == 401 || r.status == 403 => {
            eprintln!("{}", r.body);
            ExitCode::from(exit::UNAUTHORIZED)
        }
        Ok(r) if (200..300).contains(&r.status) => {
            print_success(&r.body, output);
            ExitCode::from(exit::OK)
        }
        Ok(r) => {
            if let Ok(err) = serde_json::from_str::<ErrorBody>(&r.body) {
                eprintln!("{}", err.message);
                ExitCode::from(exit_for_code(&err.code))
            } else {
                eprintln!("HTTP {}: {}", r.status, r.body);
                ExitCode::from(exit::CONFIG_INVALID)
            }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(exit::CONNECT)
        }
    }
}

fn exit_for_code(code: &str) -> u8 {
    match code {
        "NotClusterAdmin" | "CascadeRequired" | "QuotaExceeded" => exit::CONNECT,
        "NotLeader" | "Minority" => exit::UNAUTHORIZED,
        "Slice7Required" | "NamespaceNotFound" | "NamespaceExists" | "QuotaNegative"
        | "QuotaUnitUnknown" | "invalid_argument" | "unknown_op" => exit::CONFIG_INVALID,
        "unauthorized" => exit::UNAUTHORIZED,
        _ => exit::CONFIG_INVALID,
    }
}

fn print_success(body: &str, output: Option<&str>) {
    let mode = output.unwrap_or("table");
    if mode == "json" {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            crate::output::print_json(&v);
            return;
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(rows) = v.get("namespaces").and_then(|n| n.as_array()) {
            println!("NAME\tID\tQUOTAS");
            for row in rows {
                let name = row["name"].as_str().unwrap_or("");
                let id = row["id"].as_str().unwrap_or("");
                let quotas = row["quotas"]
                    .as_array()
                    .map(|q| {
                        q.iter()
                            .filter_map(|spec| {
                                let unit = spec["unit"].as_str()?;
                                let limit = spec["limit"].as_u64()?;
                                Some(format!("{unit}={limit}"))
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                println!("{name}\t{id}\t{quotas}");
            }
            return;
        }
        if v.get("roles").is_some() || v.get("namespace").is_some() || v.get("ok").is_some() {
            crate::output::print_json(&v);
            return;
        }
    }
    println!("{body}");
}
