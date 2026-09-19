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
    ConfigDiff { changed }
}
