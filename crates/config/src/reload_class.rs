use crate::model::NodeConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadClass {
    Live,
    RestartRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDiffEntry {
    pub setting: String,
    pub class: ReloadClass,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigDiff {
    pub changed: Vec<ConfigDiffEntry>,
}

pub fn diff(running: &NodeConfig, incoming: &NodeConfig) -> ConfigDiff {
    let mut changed = Vec::new();
    if running.drain_timeout != incoming.drain_timeout {
        changed.push(ConfigDiffEntry {
            setting: "runtime.drain_timeout".into(),
            class: ReloadClass::Live,
        });
    }
    if running.log_level != incoming.log_level {
        changed.push(ConfigDiffEntry {
            setting: "log.level".into(),
            class: ReloadClass::Live,
        });
    }
    if running.admin_token_file != incoming.admin_token_file {
        changed.push(ConfigDiffEntry {
            setting: "admin.token_file".into(),
            class: ReloadClass::Live,
        });
    }
    if running.buffers != incoming.buffers {
        changed.push(ConfigDiffEntry {
            setting: "buffers".into(),
            class: ReloadClass::Live,
        });
    }
    if running.threads != incoming.threads {
        changed.push(ConfigDiffEntry {
            setting: "runtime.threads".into(),
            class: ReloadClass::RestartRequired,
        });
    }
    if running.log_format != incoming.log_format {
        changed.push(ConfigDiffEntry {
            setting: "log.format".into(),
            class: ReloadClass::RestartRequired,
        });
    }
    if running.entrypoints.len() != incoming.entrypoints.len()
        || running
            .entrypoints
            .iter()
            .zip(incoming.entrypoints.iter())
            .any(|(a, b)| a.port != b.port || a.handler != b.handler || a.address != b.address)
    {
        changed.push(ConfigDiffEntry {
            setting: "entrypoints".into(),
            class: ReloadClass::RestartRequired,
        });
    }
    // 015: limits / query / product_version — Live (new connections/queries only).
    if running.limits != incoming.limits {
        changed.push(ConfigDiffEntry {
            setting: "limits".into(),
            class: ReloadClass::Live,
        });
    }
    if running.query != incoming.query {
        changed.push(ConfigDiffEntry {
            setting: "query".into(),
            class: ReloadClass::Live,
        });
    }
    if running.cluster.product_version != incoming.cluster.product_version {
        changed.push(ConfigDiffEntry {
            setting: "cluster.product_version".into(),
            class: ReloadClass::Live,
        });
    }
    ConfigDiff { changed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ClusterDecl, KeysDecl, NodeConfig, QueryDecl, QueryDefaults, StorageDecl,
    };
    use spacestorage_compat::{EffectiveLimits, Limits};
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn base() -> NodeConfig {
        NodeConfig {
            node_name: "n1".into(),
            threads: None,
            drain_timeout: Duration::from_secs(30),
            log_level: "info".into(),
            log_format: "json".into(),
            admin_token_file: None,
            disable_admin: false,
            disable_admin_http: false,
            entrypoints: vec![],
            buffers: BTreeMap::new(),
            cluster: ClusterDecl::default(),
            keys: KeysDecl::default(),
            query_defaults: QueryDefaults::default(),
            labels: BTreeMap::new(),
            storage_data_dir: None,
            storage: StorageDecl::default(),
            limits: EffectiveLimits::built_in(),
            query: QueryDecl::default(),
            metrics: crate::model::MetricsDecl::defaults(),
            log_kafka: None,
            log_syslog: None,
            jobs: crate::model::JobsDecl::default(),
        kafka_ingests: Vec::new(),
        }
    }

    #[test]
    fn limits_query_product_version_are_live() {
        let running = base();
        let mut incoming = base();
        incoming.limits = EffectiveLimits::configured(Limits {
            max_key: 4096,
            ..Limits::default()
        });
        incoming.query.max_concurrent_per_node = Some(64);
        incoming.cluster.product_version = Some(2);

        let d = diff(&running, &incoming);
        let live: Vec<_> = d
            .changed
            .iter()
            .filter(|e| e.class == ReloadClass::Live)
            .map(|e| e.setting.as_str())
            .collect();
        assert!(live.contains(&"limits"), "{live:?}");
        assert!(live.contains(&"query"), "{live:?}");
        assert!(live.contains(&"cluster.product_version"), "{live:?}");
    }
}
