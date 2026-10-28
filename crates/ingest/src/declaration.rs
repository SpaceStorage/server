//! Kafka ingest cluster object + syslog bind declaration.

use crate::error::IngestError;
use crate::format::PayloadFormat;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct IngestAuthz {
    pub cluster_admin: bool,
    pub namespace_admin: Vec<String>,
    /// "ns/container" with WRITE only (no NAMESPACE_ADMIN).
    pub write_only: Vec<String>,
    /// "ns/container" with WRITE + NAMESPACE_ADMIN or cluster.
    pub write: Vec<String>,
}

impl IngestAuthz {
    pub fn cluster_admin() -> Self {
        Self {
            cluster_admin: true,
            ..Default::default()
        }
    }

    pub fn write_only_user(ns: &str, container: &str) -> Self {
        Self {
            write_only: vec![format!("{ns}/{container}")],
            ..Default::default()
        }
    }

    pub fn namespace_admin_with_write(ns: &str, container: &str) -> Self {
        Self {
            namespace_admin: vec![ns.into()],
            write: vec![format!("{ns}/{container}")],
            ..Default::default()
        }
    }

    /// Bridge from UI / session principal fields.
    pub fn from_principal(
        cluster_admin: bool,
        namespace_admin: &[String],
        write_containers: &[String],
    ) -> Self {
        let mut write = Vec::new();
        let mut write_only = Vec::new();
        for key in write_containers {
            let ns = key.split('/').next().unwrap_or("");
            if cluster_admin || namespace_admin.iter().any(|n| n == ns) {
                write.push(key.clone());
            } else {
                write_only.push(key.clone());
            }
        }
        Self {
            cluster_admin,
            namespace_admin: namespace_admin.to_vec(),
            write_only,
            write,
        }
    }
}

/// Broker TLS file refs (never inlined PEM).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct KafkaTls {
    pub certificate: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KafkaIngest {
    pub id: Uuid,
    pub name: String,
    pub namespace: String,
    pub container: String,
    #[serde(rename = "type", default = "default_type")]
    pub type_name: String,
    pub brokers: Vec<String>,
    pub topic: String,
    pub group: String,
    #[serde(default)]
    pub format: PayloadFormat,
    /// When true, open plaintext TCP. When false, require [`Self::tls`].
    #[serde(default)]
    pub plaintext: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<KafkaTls>,
    pub created_by: Option<Uuid>,
    #[serde(default = "starting_state")]
    pub state: String,
}

fn default_type() -> String {
    "log_stream".into()
}

fn starting_state() -> String {
    "starting".into()
}

fn reject_inline_pem(s: &str) -> Result<(), IngestError> {
    if s.contains("-----BEGIN") {
        return Err(IngestError::CertInline);
    }
    Ok(())
}

impl KafkaIngest {
    pub fn validate(&self) -> Result<(), IngestError> {
        if self.namespace.is_empty() || self.container.is_empty() {
            return Err(IngestError::MissingTarget);
        }
        if self.type_name.is_empty() {
            return Err(IngestError::UnknownType);
        }
        if self.brokers.is_empty() {
            return Err(IngestError::KafkaNoBrokers);
        }
        if self.topic.is_empty() {
            return Err(IngestError::KafkaNoTopic);
        }
        if self.group.is_empty() {
            return Err(IngestError::KafkaNoGroup);
        }
        if self.plaintext {
            if self.tls.is_some() {
                return Err(IngestError::Invalid(
                    "plaintext and tls are mutually exclusive".into(),
                ));
            }
        } else {
            let Some(tls) = &self.tls else {
                return Err(IngestError::Invalid(
                    "tls { certificate; key; } required when not plaintext".into(),
                ));
            };
            reject_inline_pem(&tls.certificate)?;
            reject_inline_pem(&tls.key)?;
            if tls.certificate.is_empty() || tls.key.is_empty() {
                return Err(IngestError::Invalid(
                    "tls certificate and key paths required".into(),
                ));
            }
        }
        Ok(())
    }

    /// Whether this declaration is visible to the caller on list.
    pub fn authorize_list(&self, authz: &IngestAuthz) -> bool {
        authz.cluster_admin || authz.namespace_admin.iter().any(|n| n == &self.namespace)
    }

    pub fn authorize_mutate(&self, authz: &IngestAuthz) -> Result<(), IngestError> {
        if authz.cluster_admin {
            return Ok(());
        }
        let key = format!("{}/{}", self.namespace, self.container);
        if authz.write_only.iter().any(|k| k == &key)
            && !authz.namespace_admin.iter().any(|n| n == &self.namespace)
        {
            return Err(IngestError::WriteOnly);
        }
        let ns_ok = authz.namespace_admin.iter().any(|n| n == &self.namespace);
        let write_ok = authz.write.iter().any(|k| k == &key);
        if ns_ok && write_ok {
            return Ok(());
        }
        Err(IngestError::Forbidden)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyslogIngestBind {
    pub entrypoint: String,
    pub namespace: String,
    pub container: String,
    #[serde(rename = "type", default = "default_type")]
    pub type_name: String,
}

impl SyslogIngestBind {
    pub fn validate(&self) -> Result<(), IngestError> {
        if self.namespace.is_empty() || self.container.is_empty() {
            return Err(IngestError::MissingTarget);
        }
        Ok(())
    }

    pub fn authorize_bind(&self, authz: &IngestAuthz) -> Result<(), IngestError> {
        if authz.cluster_admin {
            Ok(())
        } else {
            Err(IngestError::SyslogClusterOnly)
        }
    }
}

/// Fired after a durable cluster-log style persist (006 apply seam).
pub type KafkaChangeHook = Arc<dyn Fn(&[KafkaIngest]) + Send + Sync>;

#[derive(Default)]
pub struct KafkaIngestStore {
    slice11: bool,
    items: Mutex<Vec<KafkaIngest>>,
    /// Durable cluster-log mirror path (JSONL of ClusterOp-shaped records).
    persist_path: Mutex<Option<PathBuf>>,
    on_change: Mutex<Option<KafkaChangeHook>>,
}

impl std::fmt::Debug for KafkaIngestStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KafkaIngestStore")
            .field("slice11", &self.slice11)
            .field("items", &*self.items.lock())
            .field("persist_path", &*self.persist_path.lock())
            .finish_non_exhaustive()
    }
}

impl KafkaIngestStore {
    pub fn new(slice11: bool) -> Self {
        Self {
            slice11,
            items: Mutex::new(Vec::new()),
            persist_path: Mutex::new(None),
            on_change: Mutex::new(None),
        }
    }

    /// Attach durable cluster-log file under `data_dir/cluster/kafka_ingest.jsonl`.
    pub fn set_cluster_log_path(&self, path: impl Into<PathBuf>) {
        *self.persist_path.lock() = Some(path.into());
    }

    pub fn set_on_change(&self, hook: KafkaChangeHook) {
        *self.on_change.lock() = Some(hook);
    }

    /// Load previously persisted declarations (node boot).
    pub fn load_from_cluster_log(&self) -> Result<(), IngestError> {
        let path = self.persist_path.lock().clone();
        let Some(path) = path else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        let text = std::fs::read_to_string(&path).map_err(|e| IngestError::Invalid(e.to_string()))?;
        let mut items = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value =
                serde_json::from_str(line).map_err(|e| IngestError::Invalid(e.to_string()))?;
            match v.get("op").and_then(|o| o.as_str()) {
                Some("upsert_kafka_ingest") => {
                    if let Some(decl) = v.get("decl") {
                        if let Ok(d) = serde_json::from_value::<KafkaIngest>(decl.clone()) {
                            items.retain(|i: &KafkaIngest| i.id != d.id && i.name != d.name);
                            items.push(d);
                        }
                    }
                }
                Some("delete_kafka_ingest") => {
                    if let Some(id) = v
                        .get("id")
                        .and_then(|x| x.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok())
                    {
                        items.retain(|i| i.id != id);
                    }
                }
                _ => {}
            }
        }
        *self.items.lock() = items;
        Ok(())
    }

    /// Append one JSONL cluster-op and `sync_all` on Tokio's blocking pool (Constitution II).
    async fn append_cluster_op(&self, op: &serde_json::Value) -> Result<(), IngestError> {
        let path = self.persist_path.lock().clone();
        let Some(path) = path else {
            return Ok(());
        };
        let line = op.to_string();
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| IngestError::Invalid(e.to_string()))?;
            }
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| IngestError::Invalid(e.to_string()))?;
            writeln!(f, "{line}").map_err(|e| IngestError::Invalid(e.to_string()))?;
            f.sync_all().map_err(|e| IngestError::Invalid(e.to_string()))?;
            Ok::<(), IngestError>(())
        })
        .await
        .map_err(|e| IngestError::Invalid(e.to_string()))?
    }

    fn notify(&self) {
        let snap = self.items.lock().clone();
        if let Some(hook) = self.on_change.lock().as_ref() {
            hook(&snap);
        }
    }

    pub fn list(&self) -> Result<Vec<KafkaIngest>, IngestError> {
        if !self.slice11 {
            return Err(IngestError::Slice11Required);
        }
        Ok(self.items.lock().clone())
    }

    /// List filtered by caller authz. Empty admin rights → forbidden.
    pub fn list_for(&self, authz: &IngestAuthz) -> Result<Vec<KafkaIngest>, IngestError> {
        if !self.slice11 {
            return Err(IngestError::Slice11Required);
        }
        if !authz.cluster_admin && authz.namespace_admin.is_empty() {
            return Err(IngestError::Forbidden);
        }
        let all = self.items.lock().clone();
        if authz.cluster_admin {
            return Ok(all);
        }
        Ok(all
            .into_iter()
            .filter(|d| d.authorize_list(authz))
            .collect())
    }

    pub async fn add(
        &self,
        mut decl: KafkaIngest,
        authz: &IngestAuthz,
    ) -> Result<KafkaIngest, IngestError> {
        if !self.slice11 {
            return Err(IngestError::Slice11Required);
        }
        if decl.id.is_nil() {
            decl.id = Uuid::now_v7();
        }
        if decl.type_name.is_empty() {
            decl.type_name = default_type();
        }
        decl.validate()?;
        decl.authorize_mutate(authz)?;
        decl.state = "running".into();
        {
            let items = self.items.lock();
            if items.iter().any(|i| i.name == decl.name) {
                return Err(IngestError::Invalid(format!(
                    "duplicate ingest name {}",
                    decl.name
                )));
            }
        }
        let op = serde_json::json!({
            "op": "upsert_kafka_ingest",
            "decl": &decl,
        });
        self.append_cluster_op(&op).await?;
        let mut items = self.items.lock();
        items.push(decl.clone());
        drop(items);
        self.notify();
        Ok(decl)
    }

    pub async fn delete(&self, id: Uuid, authz: &IngestAuthz) -> Result<(), IngestError> {
        if !self.slice11 {
            return Err(IngestError::Slice11Required);
        }
        {
            let items = self.items.lock();
            let idx = items.iter().position(|i| i.id == id).ok_or(IngestError::NotFound)?;
            items[idx].authorize_mutate(authz)?;
        }
        let op = serde_json::json!({
            "op": "delete_kafka_ingest",
            "id": id,
        });
        self.append_cluster_op(&op).await?;
        self.items.lock().retain(|i| i.id != id);
        self.notify();
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> Option<KafkaIngest> {
        self.items.lock().iter().find(|i| i.id == id).cloned()
    }

    /// Seed from bootstrap `ingest kafka` config (idempotent by name).
    pub fn seed_bootstrap(&self, decls: &[KafkaIngest]) {
        let mut items = self.items.lock();
        for d in decls {
            if !items.iter().any(|i| i.name == d.name) {
                items.push(d.clone());
            }
        }
    }

    pub fn persist_path_debug(&self) -> Option<PathBuf> {
        self.persist_path.lock().clone()
    }

    pub fn cluster_log_default_path(data_dir: &Path) -> PathBuf {
        data_dir.join("cluster").join("kafka_ingest.jsonl")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_decl(name: &str) -> KafkaIngest {
        KafkaIngest {
            id: Uuid::now_v7(),
            name: name.into(),
            namespace: "acme".into(),
            container: "events".into(),
            type_name: "log_stream".into(),
            brokers: vec!["127.0.0.1:9092".into()],
            topic: "logs".into(),
            group: "g1".into(),
            format: PayloadFormat::Raw,
            plaintext: true,
            tls: None,
            created_by: None,
            state: "starting".into(),
        }
    }

    #[test]
    fn list_for_filters_by_namespace_admin() {
        let store = KafkaIngestStore::new(true);
        {
            let mut items = store.items.lock();
            items.push(sample_decl("acme-a"));
            let mut other = sample_decl("other-b");
            other.namespace = "other".into();
            items.push(other);
        }
        let cluster = IngestAuthz::cluster_admin();
        assert_eq!(store.list_for(&cluster).unwrap().len(), 2);
        let ns = IngestAuthz {
            namespace_admin: vec!["acme".into()],
            ..Default::default()
        };
        let filtered = store.list_for(&ns).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].namespace, "acme");
        let nobody = IngestAuthz::default();
        assert_eq!(store.list_for(&nobody).unwrap_err().code(), "ingest_forbidden");
    }

    #[tokio::test]
    async fn add_delete_fsync_via_spawn_blocking() {
        let dir = tempdir().unwrap();
        let path = KafkaIngestStore::cluster_log_default_path(dir.path());
        let store = KafkaIngestStore::new(true);
        store.set_cluster_log_path(path.clone());
        let authz = IngestAuthz::cluster_admin();
        let decl = store.add(sample_decl("acme-events"), &authz).await.unwrap();
        assert!(path.exists());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("upsert_kafka_ingest"));
        store.delete(decl.id, &authz).await.unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("delete_kafka_ingest"));
        assert!(store.get(decl.id).is_none());
    }
}
