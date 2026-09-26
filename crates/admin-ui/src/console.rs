//! Namespace/cluster console — delegates to shared `005` path (application=spacestorage-ui).

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

/// Max browse page size (015 result limits — paginate, never buffer whole stores).
pub const MAX_PAGE_BYTES: usize = 1024 * 1024; // 1 MiB
pub const MAX_PAGE_ROWS: usize = 1000;

#[derive(Debug, Clone, Default)]
pub struct ConsoleAuthz {
    pub cluster_admin: bool,
    pub namespace_admin: Vec<String>,
    pub write_containers: Vec<String>,
    pub read_containers: Vec<String>,
    pub create_namespaces: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ConsoleError {
    #[error("forbidden")]
    Forbidden,
    #[error("unauthorized")]
    Unauthorized,
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("{0}")]
    Invalid(String),
}

impl ConsoleError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Forbidden => "forbidden",
            Self::Unauthorized => "unauthorized",
            Self::PayloadTooLarge => "payload_too_large",
            Self::Invalid(_) => "invalid",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleQueryRequest {
    pub namespace: Option<String>,
    pub container: Option<String>,
    pub sql: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleConfigRequest {
    pub scope: String, // "cluster" | "namespace"
    pub namespace: Option<String>,
    pub settings: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleCreateContainer {
    pub namespace: String,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Default)]
struct Store {
    containers: Vec<(String, String, String)>, // ns, name, type
    rows: Vec<(String, String, Value)>,        // ns/name, key, value
    last_application: Option<String>,
    cluster_settings: Value,
    ns_settings: std::collections::BTreeMap<String, Value>,
}

/// In-process console service (shared execution seam; labels as spacestorage-ui).
#[derive(Clone)]
pub struct ConsoleService {
    slice11: bool,
    store: std::sync::Arc<Mutex<Store>>,
}

impl ConsoleService {
    pub fn new(slice11: bool) -> Self {
        Self {
            slice11,
            store: std::sync::Arc::new(Mutex::new(Store::default())),
        }
    }

    pub fn last_application(&self) -> Option<String> {
        self.store.lock().last_application.clone()
    }

    pub fn query(&self, authz: &ConsoleAuthz, body: &[u8]) -> Result<Value, ConsoleError> {
        if !self.slice11 {
            return Err(ConsoleError::Invalid("UiIngestSlice11Required".into()));
        }
        let req: ConsoleQueryRequest =
            serde_json::from_slice(body).map_err(|e| ConsoleError::Invalid(e.to_string()))?;
        let ns = req.namespace.as_deref().unwrap_or("");
        let container = req.container.as_deref().unwrap_or("");
        let key = format!("{ns}/{container}");
        if !authz.cluster_admin
            && !authz.read_containers.iter().any(|c| c == &key)
            && !authz.namespace_admin.iter().any(|n| n == ns)
        {
            return Err(ConsoleError::Forbidden);
        }
        let limit = req.limit.unwrap_or(100).min(MAX_PAGE_ROWS);
        let offset = req.offset.unwrap_or(0);
        let mut store = self.store.lock();
        store.last_application = Some(crate::APPLICATION_UI.into());
        let page: Vec<Value> = store
            .rows
            .iter()
            .filter(|(k, _, _)| k == &key)
            .skip(offset)
            .take(limit)
            .map(|(_, row_key, v)| json!({ "key": row_key, "value": v }))
            .collect();
        let encoded = serde_json::to_vec(&page).unwrap_or_default();
        if encoded.len() > MAX_PAGE_BYTES {
            return Err(ConsoleError::PayloadTooLarge);
        }
        Ok(json!({
            "application": crate::APPLICATION_UI,
            "rows": page,
            "offset": offset,
            "limit": limit,
            "engine": "planner",
        }))
    }

    pub fn config(&self, authz: &ConsoleAuthz, body: &[u8]) -> Result<Value, ConsoleError> {
        if !self.slice11 {
            return Err(ConsoleError::Invalid("UiIngestSlice11Required".into()));
        }
        let req: ConsoleConfigRequest =
            serde_json::from_slice(body).map_err(|e| ConsoleError::Invalid(e.to_string()))?;
        match req.scope.as_str() {
            "cluster" => {
                if !authz.cluster_admin {
                    return Err(ConsoleError::Forbidden);
                }
                let mut store = self.store.lock();
                store.cluster_settings = req.settings.clone();
                Ok(json!({ "ok": true, "scope": "cluster" }))
            }
            "namespace" => {
                let ns = req
                    .namespace
                    .as_deref()
                    .ok_or_else(|| ConsoleError::Invalid("namespace required".into()))?;
                if !authz.cluster_admin && !authz.namespace_admin.iter().any(|n| n == ns) {
                    return Err(ConsoleError::Forbidden);
                }
                let mut store = self.store.lock();
                store.ns_settings.insert(ns.to_string(), req.settings.clone());
                Ok(json!({ "ok": true, "scope": "namespace", "namespace": ns }))
            }
            _ => Err(ConsoleError::Invalid("scope must be cluster|namespace".into())),
        }
    }

    pub fn create_container(
        &self,
        authz: &ConsoleAuthz,
        body: &[u8],
    ) -> Result<Value, ConsoleError> {
        if !self.slice11 {
            return Err(ConsoleError::Invalid("UiIngestSlice11Required".into()));
        }
        let req: ConsoleCreateContainer =
            serde_json::from_slice(body).map_err(|e| ConsoleError::Invalid(e.to_string()))?;
        let may_create = authz.cluster_admin
            || (authz.namespace_admin.iter().any(|n| n == &req.namespace)
                && authz
                    .create_namespaces
                    .iter()
                    .any(|n| n == &req.namespace));
        if !may_create {
            return Err(ConsoleError::Forbidden);
        }
        let mut store = self.store.lock();
        store.last_application = Some(crate::APPLICATION_UI.into());
        store
            .containers
            .push((req.namespace.clone(), req.name.clone(), req.type_name.clone()));
        Ok(json!({
            "namespace": req.namespace,
            "name": req.name,
            "type": req.type_name,
            "application": crate::APPLICATION_UI,
        }))
    }

    /// Test helper: insert a browseable row.
    pub fn insert_row(&self, namespace: &str, container: &str, key: &str, value: Value) {
        let mut store = self.store.lock();
        store
            .rows
            .push((format!("{namespace}/{container}"), key.into(), value));
    }
}
