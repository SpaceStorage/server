use crate::error::{ConfigError, ErrorCode};
use crate::model::{buffer_spec, NodeConfig, Transport, BUILTIN_BUFFERS};
use crate::parser;
use crate::resolve;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ValidateOptions {
    /// When false, skip token/cert/master-key filesystem checks (offline fixture syntax).
    pub check_secrets_readable: bool,
    /// Require every entrypoint to declare tls or plaintext (016 / first-binary).
    pub require_transport: bool,
    /// Require internode + replication entrypoints (016 first-binary).
    pub require_cluster_ports: bool,
    /// Require cluster.master_key_file when cluster block present.
    pub require_master_key: bool,
}

impl Default for ValidateOptions {
    fn default() -> Self {
        Self {
            check_secrets_readable: true,
            require_transport: false,
            require_cluster_ports: false,
            require_master_key: false,
        }
    }
}

impl ValidateOptions {
    pub fn first_binary() -> Self {
        Self {
            check_secrets_readable: true,
            require_transport: true,
            require_cluster_ports: true,
            require_master_key: true,
        }
    }

    pub fn fixtures_offline() -> Self {
        Self {
            check_secrets_readable: false,
            require_transport: false,
            require_cluster_ports: false,
            require_master_key: false,
        }
    }
}

pub fn parse_validate(
    text: &str,
    path: &Path,
    overrides: &[String],
    handler_names: &[&str],
    opts: ValidateOptions,
) -> Result<(NodeConfig, Vec<ConfigError>), Vec<ConfigError>> {
    let file = path.display().to_string();
    let doc = parser::parse(text, &file)?;
    let cfg = resolve::resolve(&doc, &file, overrides)?;
    let mut errors = validate(&cfg, &file, handler_names, &opts);
    if errors.is_empty() {
        Ok((cfg, Vec::new()))
    } else {
        // still return cfg? No — collect-all errors
        errors.sort_by(|a, b| (a.line, a.col).cmp(&(b.line, b.col)));
        Err(errors)
    }
}

pub fn validate(
    cfg: &NodeConfig,
    file: &str,
    handler_names: &[&str],
    opts: &ValidateOptions,
) -> Vec<ConfigError> {
    let mut errors = Vec::new();
    let known: BTreeSet<&str> = handler_names.iter().copied().collect();

    if cfg.node_name.is_empty() || cfg.node_name.len() > 253 {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "node.name",
            ErrorCode::BadLiteral,
            "node.name must be non-empty and <= 253 chars",
        ));
    }

    if let Some(t) = cfg.threads {
        if t < 1 {
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "runtime.threads",
                ErrorCode::ThreadsOutOfRange,
                format!("runtime.threads must be >= 1, got {t}"),
            ));
        }
    }

    let drain = cfg.drain_timeout;
    if drain < Duration::from_secs(1) || drain > Duration::from_secs(24 * 3600) {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "runtime.drain_timeout",
            ErrorCode::DrainTimeoutOutOfRange,
            format!("runtime.drain_timeout out of range {:?}", drain),
        ));
    }

    let levels = ["error", "warn", "info", "debug", "trace"];
    if !levels.contains(&cfg.log_level.as_str()) {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "log.level",
            ErrorCode::BadLiteral,
            format!("invalid log.level '{}'", cfg.log_level),
        ));
    }
    if cfg.log_format != "text" && cfg.log_format != "json" {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "log.format",
            ErrorCode::BadLiteral,
            format!("invalid log.format '{}'", cfg.log_format),
        ));
    }

    let mut admin_enabled = false;
    let mut admin_http_enabled = false;
    let mut seen_addr: BTreeMap<(String, u16), String> = BTreeMap::new();
    let mut seen_names: BTreeSet<String> = BTreeSet::new();
    let mut has_internode = false;
    let mut has_replication = false;

    for ep in &cfg.entrypoints {
        if ep.port == 0 {
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "entrypoint.port",
                ErrorCode::EntrypointMissingPort,
                format!("entrypoint '{}' has no 'port'", ep.name),
            ));
        }
        if ep.handler.is_empty() {
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "entrypoint.handler",
                ErrorCode::EntrypointMissingHandler,
                format!("entrypoint '{}' has no 'handler'", ep.name),
            ));
        } else if !known.contains(ep.handler.as_str()) {
            let mut list: Vec<&str> = handler_names.to_vec();
            list.sort();
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "entrypoint.handler",
                ErrorCode::EntrypointUnknownHandler,
                format!(
                    "entrypoint '{}': unknown handler '{}'; known handlers: {:?}",
                    ep.name, ep.handler, list
                ),
            ));
        }

        if ep.handler == "admin" {
            admin_enabled = true;
        }
        if ep.handler == "admin-http" {
            admin_http_enabled = true;
        }
        if ep.handler == "internode" {
            has_internode = true;
        }
        if ep.handler == "replication" {
            has_replication = true;
        }

        if opts.require_transport && ep.transport == Transport::Undeclared {
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "entrypoint.transport",
                ErrorCode::TransportUndeclared,
                format!(
                    "entrypoint '{}' must declare tls {{…}} or plaintext;",
                    ep.name
                ),
            ));
        }

        let key = (ep.address.clone(), ep.port);
        if let Some(prev) = seen_addr.insert(key.clone(), ep.name.clone()) {
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "entrypoint.address",
                ErrorCode::EntrypointDuplicateAddress,
                format!(
                    "entrypoint '{}' and '{}' both listen on {}:{}",
                    prev, ep.name, ep.address, ep.port
                ),
            ));
        }
        if !seen_names.insert(ep.name.clone()) {
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "entrypoint.name",
                ErrorCode::EntrypointDuplicateName,
                format!("entrypoint name '{}' used twice", ep.name),
            ));
        }

        if let Some(tls) = &ep.tls {
            if tls.certificate.contains("-----BEGIN") || tls.key.contains("-----BEGIN") {
                errors.push(ConfigError::new(
                    file,
                    0,
                    0,
                    "entrypoint.tls",
                    ErrorCode::CertInlineForbidden,
                    "certificate material must be referenced by path, not inlined",
                ));
            }
            if tls.certificate.starts_with("vault:") {
                errors.push(ConfigError::new(
                    file,
                    0,
                    0,
                    "entrypoint.tls.certificate",
                    ErrorCode::CertRefSchemeUnsupported,
                    "certificate reference scheme 'vault' is not supported in this version",
                ));
            }
        }
    }

    // Admin enable/disable rules
    if !admin_enabled && !cfg.disable_admin {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "admin",
            ErrorCode::AdminHandlerUndeclared,
            "handler 'admin' must be explicitly enabled (an entrypoint with 'handler admin;') or disabled ('disable admin;')",
        ));
    }
    if admin_enabled && cfg.disable_admin {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "admin",
            ErrorCode::AdminHandlerConflict,
            "handler 'admin' is both disabled and declared on an entrypoint",
        ));
    }
    if !admin_http_enabled && !cfg.disable_admin_http {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "admin-http",
            ErrorCode::AdminHandlerUndeclared,
            "handler 'admin-http' must be explicitly enabled or disabled ('disable admin-http;')",
        ));
    }
    if admin_http_enabled && cfg.disable_admin_http {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "admin-http",
            ErrorCode::AdminHandlerConflict,
            "handler 'admin-http' is both disabled and declared on an entrypoint",
        ));
    }

    if (admin_enabled || admin_http_enabled) && cfg.admin_token_file.is_none() {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "admin.token_file",
            ErrorCode::AdminTokenRequired,
            "admin { token_file } is required when an admin handler is enabled",
        ));
    }
    if opts.check_secrets_readable {
        if let Some(p) = &cfg.admin_token_file {
            match std::fs::read_to_string(p) {
                Ok(s) if s.trim().is_empty() => errors.push(ConfigError::new(
                    file,
                    0,
                    0,
                    "admin.token_file",
                    ErrorCode::AdminTokenUnreadable,
                    format!("cannot read admin token file '{p}': empty"),
                )),
                Ok(_) => {}
                Err(e) => errors.push(ConfigError::new(
                    file,
                    0,
                    0,
                    "admin.token_file",
                    ErrorCode::AdminTokenUnreadable,
                    format!("cannot read admin token file '{p}': {e}"),
                )),
            }
        }
    }

    for (name, cap) in &cfg.buffers {
        match buffer_spec(name) {
            None => {
                let known: Vec<&str> = BUILTIN_BUFFERS.iter().map(|(n, ..)| *n).collect();
                errors.push(ConfigError::new(
                    file,
                    0,
                    0,
                    "buffers",
                    ErrorCode::BufferUnknown,
                    format!("unknown buffer '{name}'; known buffers: {known:?}"),
                ));
            }
            Some((_, min, max)) if *cap < min || *cap > max => {
                errors.push(ConfigError::new(
                    file,
                    0,
                    0,
                    "buffers",
                    ErrorCode::BufferOutOfRange,
                    format!("buffer '{name}' capacity {cap} outside accepted range {min}..={max}"),
                ));
            }
            Some(_) => {}
        }
    }

    if opts.require_cluster_ports {
        if !has_internode {
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "entrypoint.internode",
                ErrorCode::InternodeRequired,
                "internode entrypoint is required for this profile",
            ));
        }
        if !has_replication {
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "entrypoint.replication",
                ErrorCode::ReplicationRequired,
                "replication entrypoint is required for this profile",
            ));
        }
    }

    if opts.require_master_key {
        match cfg.effective_master_key_file() {
            None if cfg.cluster.name.is_some() || cfg.cluster.bootstrap => {
                errors.push(ConfigError::new(
                    file,
                    0,
                    0,
                    "keys.master_key_file",
                    ErrorCode::MasterKeyRequired,
                    "keys { master_key_file } (or cluster alias) is required",
                ));
            }
            Some(p) if opts.check_secrets_readable => {
                let path = Path::new(p);
                // FR-008 / T051: optional laptop bootstrap generates a 32-byte 0600 master.
                if !path.exists() {
                    if cfg.keys.create_master_if_absent {
                        if let Err(e) =
                            spacestorage_crypto::MasterKey::generate_and_write_blocking(path)
                        {
                            errors.push(ConfigError::new(
                                file,
                                0,
                                0,
                                "keys.master_key_file",
                                ErrorCode::MasterKeyUnreadable,
                                format!("cannot create master key '{p}': {e}"),
                            ));
                        }
                    } else {
                        errors.push(ConfigError::new(
                            file,
                            0,
                            0,
                            "keys.master_key_file",
                            ErrorCode::MasterKeyRequired,
                            format!(
                                "master key '{p}' is missing (set create_master_if_absent to generate)"
                            ),
                        ));
                    }
                }
                if path.exists() {
                    match std::fs::metadata(path) {
                        Err(e) => {
                            errors.push(ConfigError::new(
                                file,
                                0,
                                0,
                                "keys.master_key_file",
                                ErrorCode::MasterKeyUnreadable,
                                format!("cannot read master key '{p}': {e}"),
                            ));
                        }
                        Ok(meta) => {
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                let mode = meta.permissions().mode() & 0o777;
                                if mode != 0o600 {
                                    errors.push(ConfigError::new(
                                        file,
                                        0,
                                        0,
                                        "keys.master_key_file",
                                        ErrorCode::MasterKeyPermissions,
                                        format!("master key '{p}' must be mode 0600"),
                                    ));
                                }
                            }
                            match std::fs::read(path) {
                                Err(e) => {
                                    errors.push(ConfigError::new(
                                        file,
                                        0,
                                        0,
                                        "keys.master_key_file",
                                        ErrorCode::MasterKeyUnreadable,
                                        format!("cannot read master key '{p}': {e}"),
                                    ));
                                }
                                Ok(bytes) if bytes.len() != 32 => {
                                    errors.push(ConfigError::new(
                                        file,
                                        0,
                                        0,
                                        "keys.master_key_file",
                                        ErrorCode::MasterKeyUnreadable,
                                        format!("master key '{p}' must be exactly 32 bytes"),
                                    ));
                                }
                                Ok(_) => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if cfg.cluster.topology_ladder.is_empty() && cfg.cluster.name.is_some() {
            errors.push(ConfigError::new(
                file,
                0,
                0,
                "cluster.topology_ladder",
                ErrorCode::TopologyLadderRequired,
                "topology_ladder is required after resolve",
            ));
        }
    }

    // storage.sync none refused outside SPACESTORAGE_TEST=1
    if let Some(sync) = cfg.storage.sync.as_deref() {
        if sync.eq_ignore_ascii_case("none") {
            let test = std::env::var("SPACESTORAGE_TEST").ok().as_deref() == Some("1");
            if !test {
                errors.push(ConfigError::new(
                    file,
                    0,
                    0,
                    "storage.sync",
                    ErrorCode::SyncNoneNotDurable,
                    "storage { sync none; } is not durable outside SPACESTORAGE_TEST=1",
                ));
            }
        }
    }
    if let Some(0) = cfg.storage.gc_grace_ms {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "storage.gc_grace",
            ErrorCode::GcGraceTooSmall,
            "gc_grace must be > 0",
        ));
    }

    // 015: limits zeros and product_version 0 (also caught at resolve)
    if let Err(e) = cfg.limits.limits.validate_nonzero() {
        let knob = match &e {
            spacestorage_compat::CompatError::LimitsZero { knob } => knob.clone(),
            _ => "limits".into(),
        };
        errors.push(ConfigError::new(
            file,
            0,
            0,
            format!("limits.{knob}"),
            ErrorCode::LimitsZero,
            format!("limits_zero{{knob:{knob}}}"),
        ));
    }
    if let Some(0) = cfg.cluster.product_version {
        errors.push(ConfigError::new(
            file,
            0,
            0,
            "cluster.product_version",
            ErrorCode::ProductVersionZero,
            "product_version_zero",
        ));
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClusterDecl, KeysDecl, QueryDefaults, StorageDecl};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use tempfile::tempdir;

    fn bare_cfg(master_path: &str, create_if_absent: bool) -> NodeConfig {
        NodeConfig {
            node_name: "n1".into(),
            threads: None,
            drain_timeout: Duration::from_secs(30),
            log_level: "info".into(),
            log_format: "text".into(),
            admin_token_file: None,
            disable_admin: true,
            disable_admin_http: true,
            entrypoints: Vec::new(),
            buffers: BTreeMap::new(),
            cluster: ClusterDecl {
                name: Some("c".into()),
                bootstrap: true,
                topology_ladder: vec!["node".into()],
                ..ClusterDecl::default()
            },
            keys: KeysDecl {
                master_key_file: Some(master_path.into()),
                create_master_if_absent: create_if_absent,
            },
            query_defaults: QueryDefaults::default(),
            labels: BTreeMap::new(),
            storage_data_dir: None,
            storage: StorageDecl::default(),
            limits: crate::model::EffectiveLimits::built_in(),
            query: crate::model::QueryDecl::default(),
        }
    }

    #[test]
    fn create_master_if_absent_generates_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("master.key");
        let cfg = bare_cfg(path.to_str().unwrap(), true);
        let opts = ValidateOptions {
            check_secrets_readable: true,
            require_transport: false,
            require_cluster_ports: false,
            require_master_key: true,
        };
        assert!(!path.exists());
        let errors = validate(&cfg, "test.conf", &[], &opts);
        assert!(
            errors.iter().all(|e| {
                e.code != ErrorCode::MasterKeyRequired.as_str()
                    && e.code != ErrorCode::MasterKeyUnreadable.as_str()
            }),
            "{errors:?}"
        );
        assert!(path.exists());
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 32);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn missing_master_without_flag_is_required() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("master.key");
        let cfg = bare_cfg(path.to_str().unwrap(), false);
        let opts = ValidateOptions {
            check_secrets_readable: true,
            require_transport: false,
            require_cluster_ports: false,
            require_master_key: true,
        };
        let errors = validate(&cfg, "test.conf", &[], &opts);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::MasterKeyRequired.as_str()),
            "{errors:?}"
        );
        assert!(!path.exists());
    }
}
