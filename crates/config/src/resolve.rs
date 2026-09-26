use crate::ast::{Arg, Document, Item};
use crate::error::{ConfigError, ErrorCode};
use crate::model::{
    ClusterDecl, DriveDecl, EntrypointDecl, JobsDecl, KafkaIngestDecl, KeysDecl, LogKafkaDecl,
    LogSyslogDecl, MetricsDecl, NodeConfig, QueryDecl, QueryDefaults, StorageDecl, SyslogIngestDecl,
    TlsDecl, Transport,
};
use spacestorage_compat::{EffectiveLimits, Limits};
use std::collections::BTreeMap;
use std::time::Duration;

pub fn resolve(
    doc: &Document,
    file: &str,
    overrides: &[String],
) -> Result<NodeConfig, Vec<ConfigError>> {
    let mut errors = Vec::new();
    let mut cfg = NodeConfig {
        node_name: "localhost".into(),
        threads: None,
        drain_timeout: Duration::from_secs(30),
        log_level: "info".into(),
        log_format: "text".into(),
        admin_token_file: None,
        disable_admin: false,
        disable_admin_http: false,
        entrypoints: Vec::new(),
        buffers: BTreeMap::new(),
        cluster: ClusterDecl::default(),
        keys: KeysDecl::default(),
        query_defaults: QueryDefaults::default(),
        labels: BTreeMap::new(),
        storage_data_dir: None,
        storage: StorageDecl::default(),
        limits: EffectiveLimits::built_in(),
        query: QueryDecl::default(),
        metrics: MetricsDecl::defaults(),
        log_kafka: None,
        log_syslog: None,
        jobs: JobsDecl::default(),
        kafka_ingests: Vec::new(),
    };

    for item in &doc.items {
        match item {
            Item::Directive(d) if d.name == "disable" => {
                if d.args.len() != 1 {
                    errors.push(ConfigError::new(
                        file,
                        d.line,
                        d.col,
                        "disable",
                        ErrorCode::WrongArity,
                        format!("'disable' expects 1 argument(s), got {}", d.args.len()),
                    ));
                    continue;
                }
                match d.args[0].as_str().as_str() {
                    "admin" => cfg.disable_admin = true,
                    "admin-http" => cfg.disable_admin_http = true,
                    other => errors.push(ConfigError::new(
                        file,
                        d.line,
                        d.col,
                        "disable",
                        ErrorCode::BadLiteral,
                        format!("unknown disable target '{other}'"),
                    )),
                }
            }
            Item::Block(b) => match b.name.as_str() {
                "node" => {
                    for it in &b.items {
                        if let Item::Directive(d) = it {
                            if d.name == "name" {
                                if let Some(a) = d.args.first() {
                                    cfg.node_name = a.as_str();
                                }
                            } else if d.name == "labels" {
                                // nested handled below
                            } else {
                                // ignore unknown for now inside node? labels is block
                            }
                        } else if let Item::Block(lb) = it {
                            if lb.name == "labels" {
                                for lit in &lb.items {
                                    if let Item::Directive(d) = lit {
                                        if let Some(v) = d.args.first() {
                                            cfg.labels.insert(d.name.clone(), v.as_str());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                "runtime" => {
                    for it in &b.items {
                        if let Item::Directive(d) = it {
                            match d.name.as_str() {
                                "threads" => {
                                    if let Some(Arg::Ident(s)) = d.args.first() {
                                        if s == "auto" {
                                            cfg.threads = None;
                                        }
                                    } else if let Some(Arg::Number(n)) = d.args.first() {
                                        cfg.threads = Some(*n as u32);
                                    }
                                }
                                "drain_timeout" => {
                                    if let Some(Arg::DurationMs(ms)) = d.args.first() {
                                        cfg.drain_timeout = Duration::from_millis(*ms);
                                    } else if let Some(Arg::Number(n)) = d.args.first() {
                                        // bare number as seconds
                                        cfg.drain_timeout = Duration::from_secs(*n);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "log" => {
                    for it in &b.items {
                        match it {
                            Item::Directive(d) => match d.name.as_str() {
                                "level" => {
                                    if let Some(a) = d.args.first() {
                                        cfg.log_level = a.as_str();
                                    }
                                }
                                "format" => {
                                    if let Some(a) = d.args.first() {
                                        cfg.log_format = a.as_str();
                                    }
                                }
                                _ => {}
                            },
                            Item::Block(sb) if sb.name == "kafka" => {
                                let mut brokers = Vec::new();
                                let mut topic = String::new();
                                for sit in &sb.items {
                                    if let Item::Directive(d) = sit {
                                        match d.name.as_str() {
                                            "brokers" => {
                                                brokers = d
                                                    .args
                                                    .iter()
                                                    .map(|a| a.as_str())
                                                    .filter(|s| !s.is_empty())
                                                    .collect();
                                            }
                                            "topic" => {
                                                if let Some(a) = d.args.first() {
                                                    topic = a.as_str();
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                cfg.log_kafka = Some(LogKafkaDecl { brokers, topic });
                            }
                            Item::Block(sb) if sb.name == "syslog" => {
                                let mut address = String::new();
                                let mut transport = "udp".into();
                                for sit in &sb.items {
                                    if let Item::Directive(d) = sit {
                                        match d.name.as_str() {
                                            "address" => {
                                                if let Some(a) = d.args.first() {
                                                    address = a.as_str();
                                                }
                                            }
                                            "transport" => {
                                                if let Some(a) = d.args.first() {
                                                    transport = a.as_str();
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                cfg.log_syslog = Some(LogSyslogDecl { address, transport });
                            }
                            _ => {}
                        }
                    }
                }
                "metrics" => {
                    cfg.metrics = MetricsDecl::defaults();
                    for it in &b.items {
                        match it {
                            Item::Directive(d) if d.name == "audit_log" => {
                                if let Some(a) = d.args.first() {
                                    let v = a.as_str();
                                    cfg.metrics.audit_log = v == "on" || v == "true" || v == "1";
                                }
                            }
                            Item::Block(sb) if sb.name == "otel" => {
                                for sit in &sb.items {
                                    if let Item::Directive(d) = sit {
                                        match d.name.as_str() {
                                            "endpoint" => {
                                                if let Some(a) = d.args.first() {
                                                    cfg.metrics.otel_endpoint = Some(a.as_str());
                                                }
                                            }
                                            "interval" => {
                                                if let Some(a) = d.args.first() {
                                                    let s = a.as_str();
                                                    if let Ok(secs) =
                                                        s.trim_end_matches('s').parse::<u64>()
                                                    {
                                                        cfg.metrics.otel_interval =
                                                            Some(Duration::from_secs(secs));
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            Item::Block(sb) if sb.name == "slow_query" => {
                                for sit in &sb.items {
                                    if let Item::Directive(d) = sit {
                                        match d.name.as_str() {
                                            "enabled" => {
                                                if let Some(a) = d.args.first() {
                                                    let v = a.as_str();
                                                    cfg.metrics.slow_query_enabled =
                                                        v == "on" || v == "true" || v == "1";
                                                }
                                            }
                                            "threshold" => {
                                                if let Some(a) = d.args.first() {
                                                    let s = a.as_str();
                                                    if let Ok(secs) =
                                                        s.trim_end_matches('s').parse::<u64>()
                                                    {
                                                        cfg.metrics.slow_query_threshold =
                                                            Duration::from_secs(secs.max(1));
                                                    } else if let Some(Arg::Number(n)) =
                                                        d.args.first()
                                                    {
                                                        cfg.metrics.slow_query_threshold =
                                                            Duration::from_secs(*n);
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "admin" => {
                    for it in &b.items {
                        if let Item::Directive(d) = it {
                            if d.name == "token_file" {
                                if let Some(a) = d.args.first() {
                                    cfg.admin_token_file = Some(a.as_str());
                                }
                            }
                        }
                    }
                }
                "entrypoint" => {
                    let mut ep = EntrypointDecl {
                        name: b
                            .args
                            .first()
                            .map(|a| a.as_str())
                            .unwrap_or_default(),
                        address: "127.0.0.1".into(),
                        port: 0,
                        handler: String::new(),
                        transport: Transport::Undeclared,
                        tls: None,
                        ingest: None,
                    };
                    let mut handler_count = 0u32;
                    for it in &b.items {
                        match it {
                            Item::Directive(d) => match d.name.as_str() {
                                "address" => {
                                    if let Some(a) = d.args.first() {
                                        ep.address = a.as_str();
                                    }
                                }
                                "port" => {
                                    if let Some(Arg::Number(n)) = d.args.first() {
                                        ep.port = *n as u16;
                                    }
                                }
                                "handler" => {
                                    handler_count += 1;
                                    if let Some(a) = d.args.first() {
                                        ep.handler = a.as_str();
                                    }
                                }
                                "plaintext" => {
                                    ep.transport = Transport::Plaintext;
                                }
                                _ => {}
                            },
                            Item::Block(tb) if tb.name == "tls" => {
                                ep.transport = Transport::Tls;
                                let mut cert = None;
                                let mut key = None;
                                for tit in &tb.items {
                                    if let Item::Directive(d) = tit {
                                        match d.name.as_str() {
                                            "certificate" => {
                                                cert = d.args.first().map(|a| a.as_str())
                                            }
                                            "key" => key = d.args.first().map(|a| a.as_str()),
                                            _ => {}
                                        }
                                    }
                                }
                                if let (Some(c), Some(k)) = (cert, key) {
                                    ep.tls = Some(TlsDecl {
                                        certificate: c,
                                        key: k,
                                    });
                                }
                            }
                            Item::Block(ib) if ib.name == "ingest" => {
                                let mut ns = String::new();
                                let mut container = String::new();
                                let mut type_name = "log_stream".into();
                                for iit in &ib.items {
                                    if let Item::Directive(d) = iit {
                                        match d.name.as_str() {
                                            "namespace" => {
                                                if let Some(a) = d.args.first() {
                                                    ns = a.as_str();
                                                }
                                            }
                                            "container" => {
                                                if let Some(a) = d.args.first() {
                                                    container = a.as_str();
                                                }
                                            }
                                            "type" => {
                                                if let Some(a) = d.args.first() {
                                                    type_name = a.as_str();
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                ep.ingest = Some(SyslogIngestDecl {
                                    namespace: ns,
                                    container,
                                    type_name,
                                });
                            }
                            _ => {}
                        }
                    }
                    if ep.name.is_empty() {
                        ep.name = format!("{}@{}:{}", ep.handler, ep.address, ep.port);
                    }
                    if handler_count > 1 {
                        errors.push(ConfigError::new(
                            file,
                            b.line,
                            b.col,
                            "entrypoint.handler",
                            ErrorCode::EntrypointMultipleHandlers,
                            format!("entrypoint '{}' declares more than one handler", ep.name),
                        ));
                    }
                    cfg.entrypoints.push(ep);
                }
                "buffers" => {
                    for it in &b.items {
                        if let Item::Directive(d) = it {
                            let size = match d.args.first() {
                                Some(Arg::Size(n)) => *n,
                                Some(Arg::Number(n)) => *n,
                                _ => 0,
                            };
                            cfg.buffers.insert(d.name.clone(), size);
                        }
                    }
                }
                "cluster" => {
                    for it in &b.items {
                        match it {
                            Item::Directive(d) => {
                                match d.name.as_str() {
                                    "name" => {
                                        cfg.cluster.name = d.args.first().map(|a| a.as_str());
                                    }
                                    "bootstrap" => cfg.cluster.bootstrap = true,
                                    "topology_ladder" => {
                                        cfg.cluster.topology_ladder =
                                            d.args.iter().map(|a| a.as_str()).collect();
                                    }
                                    "quorum_domain" => {
                                        cfg.cluster.quorum_domain =
                                            d.args.first().map(|a| a.as_str());
                                    }
                                    "master_key_file" => {
                                        cfg.cluster.master_key_file =
                                            d.args.first().map(|a| a.as_str());
                                    }
                                    "token_file" => {
                                        cfg.cluster.token_file =
                                            d.args.first().map(|a| a.as_str());
                                    }
                                    "join" => {
                                        // Flag: `join;` → Some(""); optional arg preserved.
                                        cfg.cluster.join = Some(
                                            d.args
                                                .first()
                                                .map(|a| a.as_str())
                                                .unwrap_or_default(),
                                        );
                                    }
                                    "join_token_file" => {
                                        cfg.cluster.join_token_file =
                                            d.args.first().map(|a| a.as_str());
                                    }
                                    "admin_login" => {
                                        cfg.cluster.admin_login =
                                            d.args.first().map(|a| a.as_str());
                                    }
                                    "admin_password_file" => {
                                        cfg.cluster.admin_password_file =
                                            d.args.first().map(|a| a.as_str());
                                    }
                                    "product_version" => match d.args.first() {
                                        Some(Arg::Number(0)) => {
                                            errors.push(ConfigError::new(
                                                file,
                                                d.line,
                                                d.col,
                                                "cluster.product_version",
                                                ErrorCode::ProductVersionZero,
                                                "product_version_zero",
                                            ));
                                        }
                                        Some(Arg::Number(n)) => {
                                            cfg.cluster.product_version = Some(*n as u16);
                                        }
                                        _ => {}
                                    },
                                    "controller_exclusive_data" => {
                                        let on = d
                                            .args
                                            .first()
                                            .map(|a| {
                                                matches!(a.as_str().as_str(), "on" | "true" | "1")
                                            })
                                            .unwrap_or(true);
                                        cfg.cluster.controller_exclusive_data = on;
                                    }
                                    _ => {}
                                }
                            }
                            Item::Block(sb) if sb.name == "seeds" || sb.name == "peers" => {
                                let mut seed = crate::model::SeedDecl::default();
                                for sit in &sb.items {
                                    if let Item::Directive(sd) = sit {
                                        match sd.name.as_str() {
                                            "name" => {
                                                seed.name = sd
                                                    .args
                                                    .first()
                                                    .map(|a| a.as_str())
                                                    .unwrap_or_default();
                                            }
                                            "address" => {
                                                seed.address = sd
                                                    .args
                                                    .first()
                                                    .map(|a| a.as_str())
                                                    .unwrap_or_default();
                                            }
                                            "port" => {
                                                if let Some(Arg::Number(n)) = sd.args.first() {
                                                    seed.port = *n as u16;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                if !seed.address.is_empty() && seed.port != 0 {
                                    cfg.cluster.seeds.push(seed);
                                }
                            }
                            Item::Block(sb) if sb.name == "raft" => {
                                for sit in &sb.items {
                                    if let Item::Directive(sd) = sit {
                                        match sd.name.as_str() {
                                            "heartbeat" => {
                                                if let Some(Arg::DurationMs(ms)) = sd.args.first() {
                                                    cfg.cluster.raft.heartbeat =
                                                        Duration::from_millis(*ms);
                                                } else if let Some(Arg::Number(n)) = sd.args.first()
                                                {
                                                    cfg.cluster.raft.heartbeat =
                                                        Duration::from_millis(*n);
                                                }
                                            }
                                            "election_timeout" => {
                                                if let Some(Arg::DurationMs(ms)) = sd.args.first() {
                                                    cfg.cluster.raft.election_timeout =
                                                        Duration::from_millis(*ms);
                                                } else if let Some(Arg::Number(n)) = sd.args.first()
                                                {
                                                    cfg.cluster.raft.election_timeout =
                                                        Duration::from_secs(*n);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            Item::Block(_) => {}
                        }
                    }
                    // Validate raft timeouts after cluster block.
                    if cfg.cluster.raft.heartbeat.is_zero()
                        || cfg.cluster.raft.election_timeout.is_zero()
                        || cfg.cluster.raft.election_timeout <= cfg.cluster.raft.heartbeat
                    {
                        errors.push(ConfigError::new(
                            file,
                            b.line,
                            b.col,
                            "cluster.raft",
                            ErrorCode::RaftTimeoutInvalid,
                            "election_timeout must be > heartbeat and both must be > 0",
                        ));
                    }
                    if cfg.cluster.controller_exclusive_data {
                        // First-binary / without controlplane-ops: refuse at validate.
                        #[cfg(not(feature = "controlplane-ops"))]
                        {
                            errors.push(ConfigError::new(
                                file,
                                b.line,
                                b.col,
                                "cluster.controller_exclusive_data",
                                ErrorCode::Slice7Required,
                                "controller_exclusive_data on requires controlplane-ops (slice 7)",
                            ));
                        }
                    }
                }
                "limits" => {
                    let mut lim = Limits::default();
                    for it in &b.items {
                        if let Item::Directive(d) = it {
                            let size_or_num = |args: &[Arg]| -> Result<Option<u64>, ()> {
                                match args.first() {
                                    Some(Arg::Size(n)) | Some(Arg::Number(n)) => Ok(Some(*n)),
                                    Some(Arg::Ident(_)) | Some(Arg::String(_)) => Err(()),
                                    _ => Ok(None),
                                }
                            };
                            match d.name.as_str() {
                                "max_key" => match size_or_num(&d.args) {
                                    Ok(Some(n)) => lim.max_key = n,
                                    Err(()) => errors.push(ConfigError::new(
                                        file,
                                        d.line,
                                        d.col,
                                        "limits.max_key",
                                        ErrorCode::LimitsUnknownUnit,
                                        "limits_unknown_unit",
                                    )),
                                    Ok(None) => {}
                                },
                                "max_value" => match size_or_num(&d.args) {
                                    Ok(Some(n)) => lim.max_value = n,
                                    Err(()) => errors.push(ConfigError::new(
                                        file,
                                        d.line,
                                        d.col,
                                        "limits.max_value",
                                        ErrorCode::LimitsUnknownUnit,
                                        "limits_unknown_unit",
                                    )),
                                    Ok(None) => {}
                                },
                                "max_query_text" => match size_or_num(&d.args) {
                                    Ok(Some(n)) => lim.max_query_text = n,
                                    Err(()) => errors.push(ConfigError::new(
                                        file,
                                        d.line,
                                        d.col,
                                        "limits.max_query_text",
                                        ErrorCode::LimitsUnknownUnit,
                                        "limits_unknown_unit",
                                    )),
                                    Ok(None) => {}
                                },
                                "max_result" => match size_or_num(&d.args) {
                                    Ok(Some(n)) => lim.max_result = n,
                                    Err(()) => errors.push(ConfigError::new(
                                        file,
                                        d.line,
                                        d.col,
                                        "limits.max_result",
                                        ErrorCode::LimitsUnknownUnit,
                                        "limits_unknown_unit",
                                    )),
                                    Ok(None) => {}
                                },
                                "max_connections_per_entrypoint" => {
                                    if let Some(Arg::Number(n)) = d.args.first() {
                                        lim.max_connections_per_entrypoint = *n as u32;
                                    }
                                }
                                "max_connections_per_principal" => {
                                    if let Some(Arg::Number(n)) = d.args.first() {
                                        lim.max_connections_per_principal = *n as u32;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if let Err(e) = lim.validate_nonzero() {
                        let knob = match &e {
                            spacestorage_compat::CompatError::LimitsZero { knob } => knob.clone(),
                            _ => "limits".into(),
                        };
                        errors.push(ConfigError::new(
                            file,
                            b.line,
                            b.col,
                            format!("limits.{knob}"),
                            ErrorCode::LimitsZero,
                            format!("limits_zero{{knob:{knob}}}"),
                        ));
                    }
                    cfg.limits = EffectiveLimits::configured(lim);
                }
                "query" => {
                    for it in &b.items {
                        if let Item::Directive(d) = it {
                            match d.name.as_str() {
                                "max_concurrent_per_node" => {
                                    if let Some(Arg::Number(n)) = d.args.first() {
                                        cfg.query.max_concurrent_per_node = Some(*n as u32);
                                    }
                                }
                                "max_concurrent_per_namespace" => {
                                    if let Some(Arg::Number(n)) = d.args.first() {
                                        cfg.query.max_concurrent_per_namespace = Some(*n as u32);
                                    }
                                }
                                "max_memory" => {
                                    if let Some(Arg::Size(n)) = d.args.first() {
                                        cfg.query.max_memory = Some(*n);
                                    } else if let Some(Arg::Number(n)) = d.args.first() {
                                        cfg.query.max_memory = Some(*n);
                                    }
                                }
                                "spill" => {
                                    match d.args.first().map(|a| a.as_str().to_ascii_lowercase()) {
                                        Some(s) if s == "on" || s == "true" => {
                                            cfg.query.spill = Some(true);
                                        }
                                        Some(s) if s == "off" || s == "false" => {
                                            cfg.query.spill = Some(false);
                                        }
                                        None if d.args.is_empty() => {
                                            cfg.query.spill = Some(true);
                                        }
                                        Some(_) => {
                                            errors.push(ConfigError::new(
                                                file,
                                                d.line,
                                                d.col,
                                                "query.spill",
                                                ErrorCode::QuerySpillUnknown,
                                                "query_spill_unknown",
                                            ));
                                        }
                                        None => {}
                                    }
                                }
                                "default_concurrency" => {
                                    if let Some(Arg::Number(n)) = d.args.first() {
                                        cfg.query.default_concurrency = Some(*n as u16);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "keys" => {
                    for it in &b.items {
                        if let Item::Directive(d) = it {
                            match d.name.as_str() {
                                "master_key_file" => {
                                    cfg.keys.master_key_file = d.args.first().map(|a| a.as_str());
                                }
                                "create_master_if_absent" => {
                                    cfg.keys.create_master_if_absent = true;
                                }
                                "keyring_file" => {
                                    errors.push(ConfigError::new(
                                        file,
                                        d.line,
                                        d.col,
                                        "keys.keyring_file",
                                        ErrorCode::KeyringRemoved,
                                        "keys { keyring_file } removed; use master_key_file",
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "query_defaults" => {
                    for it in &b.items {
                        if let Item::Directive(d) = it {
                            match d.name.as_str() {
                                "write_quorum" => {
                                    if let Some(a) = d.args.first() {
                                        cfg.query_defaults.write_quorum = a.as_str();
                                    }
                                }
                                "read_quorum" => {
                                    if let Some(a) = d.args.first() {
                                        cfg.query_defaults.read_quorum = a.as_str();
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "storage" => {
                    for it in &b.items {
                        match it {
                            Item::Directive(d) => match d.name.as_str() {
                                "data_dir" => {
                                    cfg.storage_data_dir = d.args.first().map(|a| a.as_str());
                                }
                                "sync" => {
                                    cfg.storage.sync = d.args.first().map(|a| a.as_str());
                                }
                                "gc_grace" => {
                                    if let Some(Arg::DurationMs(ms)) = d.args.first() {
                                        cfg.storage.gc_grace_ms = Some(*ms);
                                    }
                                }
                                "wal_segment" => {
                                    if let Some(Arg::Size(n)) = d.args.first() {
                                        cfg.storage.wal_segment_bytes = Some(*n);
                                    } else if let Some(Arg::Number(n)) = d.args.first() {
                                        cfg.storage.wal_segment_bytes = Some(*n);
                                    }
                                }
                                _ => {}
                            },
                            Item::Block(sb) if sb.name == "group_commit" => {
                                for git in &sb.items {
                                    if let Item::Directive(d) = git {
                                        match d.name.as_str() {
                                            "max_wait" => {
                                                if let Some(Arg::DurationMs(ms)) = d.args.first() {
                                                    cfg.storage.group_commit_max_wait_ms = Some(*ms);
                                                }
                                            }
                                            "max_bytes" => {
                                                if let Some(Arg::Size(n)) = d.args.first() {
                                                    cfg.storage.group_commit_max_bytes = Some(*n);
                                                } else if let Some(Arg::Number(n)) = d.args.first()
                                                {
                                                    cfg.storage.group_commit_max_bytes = Some(*n);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            Item::Block(sb) if sb.name == "drive" => {
                                let mut id = String::new();
                                let mut path = String::new();
                                for dit in &sb.items {
                                    if let Item::Directive(d) = dit {
                                        match d.name.as_str() {
                                            "id" | "name" => {
                                                id = d
                                                    .args
                                                    .first()
                                                    .map(|a| a.as_str())
                                                    .unwrap_or_default();
                                            }
                                            "path" => {
                                                path = d
                                                    .args
                                                    .first()
                                                    .map(|a| a.as_str())
                                                    .unwrap_or_default();
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                // drive "name" { path … } — name may be block arg
                                if id.is_empty() {
                                    if let Some(Arg::Ident(n)) = sb.args.first() {
                                        id = n.clone();
                                    } else if let Some(a) = sb.args.first() {
                                        id = a.as_str();
                                    }
                                }
                                if !path.is_empty() {
                                    if id.is_empty() {
                                        id = format!("drive{}", cfg.storage.drives.len());
                                    }
                                    cfg.storage.drives.push(DriveDecl { id, path });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // reserved foreign blocks accepted for first-binary fixtures / later features
                "labels" | "memory" | "namespace" | "replication" | "types" => {}
                "ingest" => {
                    // `ingest kafka NAME { … }` — block name is ingest, first arg may be "kafka"
                    let kind = b.args.first().map(|a| a.as_str()).unwrap_or_default();
                    if kind == "kafka" {
                        let mut decl = KafkaIngestDecl {
                            name: b.args.get(1).map(|a| a.as_str()).unwrap_or_default(),
                            ..KafkaIngestDecl::default()
                        };
                        for it in &b.items {
                            match it {
                                Item::Directive(d) => match d.name.as_str() {
                                    "namespace" => {
                                        if let Some(a) = d.args.first() {
                                            decl.namespace = a.as_str();
                                        }
                                    }
                                    "container" => {
                                        if let Some(a) = d.args.first() {
                                            decl.container = a.as_str();
                                        }
                                    }
                                    "type" => {
                                        if let Some(a) = d.args.first() {
                                            decl.type_name = a.as_str();
                                        }
                                    }
                                    "format" => {
                                        if let Some(a) = d.args.first() {
                                            decl.format = a.as_str();
                                        }
                                    }
                                    "brokers" => {
                                        if let Some(a) = d.args.first() {
                                            decl.brokers.push(a.as_str());
                                        }
                                    }
                                    "topic" => {
                                        if let Some(a) = d.args.first() {
                                            decl.topic = a.as_str();
                                        }
                                    }
                                    "group" => {
                                        if let Some(a) = d.args.first() {
                                            decl.group = a.as_str();
                                        }
                                    }
                                    "plaintext" => decl.plaintext = true,
                                    _ => {}
                                },
                                Item::Block(tb) if tb.name == "tls" => {
                                    decl.plaintext = false;
                                }
                                _ => {}
                            }
                        }
                        cfg.kafka_ingests.push(decl);
                    }
                }
                "jobs" => {
                    cfg.jobs = JobsDecl::default();
                    // Slice-10 compile default: enabled on when migration-backup feature is selected
                    // at process start; config can still flip explicitly.
                    #[cfg(feature = "migration-backup")]
                    {
                        cfg.jobs.enabled = true;
                    }
                    for it in &b.items {
                        if let Item::Directive(d) = it {
                            match d.name.as_str() {
                                "enabled" => {
                                    if let Some(a) = d.args.first() {
                                        let v = a.as_str();
                                        cfg.jobs.enabled =
                                            v == "on" || v == "true" || v == "1";
                                    }
                                }
                                "catchup_concurrency" => {
                                    if let Some(Arg::Number(n)) = d.args.first() {
                                        cfg.jobs.catchup_concurrency = (*n).max(1) as u32;
                                    } else if let Some(a) = d.args.first() {
                                        if let Ok(n) = a.as_str().parse::<u32>() {
                                            cfg.jobs.catchup_concurrency = n.max(1);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                other => {
                    errors.push(ConfigError::new(
                        file,
                        b.line,
                        b.col,
                        other,
                        ErrorCode::UnknownDirective,
                        format!("unknown directive '{other}' at {file}:{}:{}", b.line, b.col),
                    ));
                }
            },
            Item::Directive(d) => {
                errors.push(ConfigError::new(
                    file,
                    d.line,
                    d.col,
                    &d.name,
                    ErrorCode::UnknownDirective,
                    format!(
                        "unknown directive '{}' at {file}:{}:{}",
                        d.name, d.line, d.col
                    ),
                ));
            }
        }
    }

    // Apply --set overrides (dotted.path=value)
    for ov in overrides {
        if let Some((k, v)) = ov.split_once('=') {
            match k {
                "runtime.threads" => {
                    if v == "auto" {
                        cfg.threads = None;
                    } else if let Ok(n) = v.parse::<u32>() {
                        cfg.threads = Some(n);
                    }
                }
                "runtime.drain_timeout" => {
                    if let Ok(secs) = v.trim_end_matches('s').parse::<u64>() {
                        cfg.drain_timeout = Duration::from_secs(secs);
                    }
                }
                "log.level" => cfg.log_level = v.to_string(),
                other if other.starts_with("buffers.") => {
                    let name = &other["buffers.".len()..];
                    if let Ok(n) = parse_size_override(v) {
                        cfg.buffers.insert(name.to_string(), n);
                    }
                }
                _ => {}
            }
        }
    }

    // Default topology ladder [az]
    if cfg.cluster.topology_ladder.is_empty() && cfg.cluster.name.is_some() {
        cfg.cluster.topology_ladder = vec!["az".into()];
    }

    if errors.is_empty() {
        Ok(cfg)
    } else {
        Err(errors)
    }
}

fn parse_size_override(v: &str) -> Result<u64, ()> {
    let v = v.trim();
    if let Some(n) = v.strip_suffix(['m', 'M']) {
        return Ok(n.parse::<u64>().map_err(|_| ())? * 1024 * 1024);
    }
    if let Some(n) = v.strip_suffix(['k', 'K']) {
        return Ok(n.parse::<u64>().map_err(|_| ())? * 1024);
    }
    if let Some(n) = v.strip_suffix(['g', 'G']) {
        return Ok(n.parse::<u64>().map_err(|_| ())? * 1024 * 1024 * 1024);
    }
    v.parse().map_err(|_| ())
}
