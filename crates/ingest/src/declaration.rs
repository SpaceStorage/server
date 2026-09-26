//! Kafka ingest cluster object + syslog bind declaration.

use crate::error::IngestError;
use crate::format::PayloadFormat;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
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
    #[serde(default)]
    pub plaintext: bool,
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
        Ok(())
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

#[derive(Debug, Default)]
pub struct KafkaIngestStore {
    slice11: bool,
    items: Mutex<Vec<KafkaIngest>>,
}

impl KafkaIngestStore {
    pub fn new(slice11: bool) -> Self {
        Self {
            slice11,
            items: Mutex::new(Vec::new()),
        }
    }

    pub fn list(&self) -> Result<Vec<KafkaIngest>, IngestError> {
        if !self.slice11 {
            return Err(IngestError::Slice11Required);
        }
        Ok(self.items.lock().clone())
    }

    pub fn add(&self, mut decl: KafkaIngest, authz: &IngestAuthz) -> Result<KafkaIngest, IngestError> {
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
        let mut items = self.items.lock();
        if items.iter().any(|i| i.name == decl.name) {
            return Err(IngestError::Invalid(format!(
                "duplicate ingest name {}",
                decl.name
            )));
        }
        items.push(decl.clone());
        Ok(decl)
    }

    pub fn delete(&self, id: Uuid, authz: &IngestAuthz) -> Result<(), IngestError> {
        if !self.slice11 {
            return Err(IngestError::Slice11Required);
        }
        let mut items = self.items.lock();
        let idx = items.iter().position(|i| i.id == id).ok_or(IngestError::NotFound)?;
        items[idx].authorize_mutate(authz)?;
        items.remove(idx);
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> Option<KafkaIngest> {
        self.items.lock().iter().find(|i| i.id == id).cloned()
    }
}
