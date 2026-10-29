//! Console → shared `005` planner + `001`/`006`/`007` admin config surfaces (009 T050/T060).

use crate::Node;
use serde_json::{Value, json};
use spacestorage_admin_ui::{
    APPLICATION_UI, ConsoleAuthz, ConsoleBackend, ConsoleConfigRequest, ConsoleCreateContainer,
    ConsoleError, ConsoleQueryRequest, MAX_PAGE_BYTES, MAX_PAGE_ROWS,
};
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

pub struct CatalogConsoleBackend {
    pub node: Arc<Node>,
    /// Namespace-scoped settings (006/007 surface). Cluster-admin or that NAMESPACE_ADMIN.
    ns_settings: parking_lot::Mutex<BTreeMap<String, Value>>,
}

impl CatalogConsoleBackend {
    pub fn new(node: Arc<Node>) -> Self {
        Self {
            node,
            ns_settings: parking_lot::Mutex::new(BTreeMap::new()),
        }
    }

    fn engine(&self) -> PlannerEngine {
        PlannerEngine::with_distributed(Arc::clone(&self.node.catalog), true)
            .with_tenancy(Arc::clone(&self.node.tenancy))
    }

    fn query_options(&self, authz: &ConsoleAuthz) -> spacestorage_query::QueryOptions {
        let mut options = spacestorage_query::QueryOptions::default();
        if !authz.principal.is_empty() {
            options.principal = Some(authz.principal.clone());
        }
        options
    }

    fn model_for_type(type_name: &str) -> &'static str {
        match type_name {
            "log_stream" | "document_store" | "doc" | "document" => "doc",
            "kv_store" | "kv" | "KvStore" => "kv",
            _ => "table",
        }
    }

    fn cluster_settings_view(&self) -> Value {
        let cfg = self.node.config.load();
        json!({
            "node_name": cfg.node_name,
            "log.level": cfg.log_level,
            "runtime.drain_timeout": cfg.drain_timeout.as_secs(),
            "buffers": cfg.buffers,
            "admin.token_file": cfg.admin_token_file,
        })
    }

    /// Apply live `001` settings the admin surface already exposes.
    fn apply_cluster_settings(&self, settings: &Value) -> Result<Vec<String>, ConsoleError> {
        let obj = settings
            .as_object()
            .ok_or_else(|| ConsoleError::Invalid("settings must be an object".into()))?;
        let mut applied = Vec::new();
        let running = self.node.config.load();
        let mut next = (**running).clone();
        for (k, v) in obj {
            match k.as_str() {
                "log.level" | "log_level" => {
                    let level = v
                        .as_str()
                        .ok_or_else(|| ConsoleError::Invalid("log.level must be string".into()))?;
                    next.log_level = level.to_string();
                    crate::logging::apply_level(level);
                    applied.push("log.level".into());
                }
                "runtime.drain_timeout" | "drain_timeout" => {
                    let secs = v.as_u64().ok_or_else(|| {
                        ConsoleError::Invalid("runtime.drain_timeout must be u64 seconds".into())
                    })?;
                    next.drain_timeout = Duration::from_secs(secs);
                    applied.push("runtime.drain_timeout".into());
                }
                "buffers" => {
                    let map = v.as_object().ok_or_else(|| {
                        ConsoleError::Invalid("buffers must be an object of name→bytes".into())
                    })?;
                    for (name, cap) in map {
                        let n = cap.as_u64().ok_or_else(|| {
                            ConsoleError::Invalid(format!("buffer '{name}' capacity must be u64"))
                        })?;
                        next.buffers.insert(name.clone(), n);
                    }
                    self.node.buffers.apply_capacities(&next);
                    applied.push("buffers".into());
                }
                "admin.token_file" => {
                    let path = match v {
                        Value::Null => None,
                        Value::String(s) => Some(s.clone()),
                        _ => {
                            return Err(ConsoleError::Invalid(
                                "admin.token_file must be string or null".into(),
                            ));
                        }
                    };
                    next.admin_token_file = path;
                    applied.push("admin.token_file".into());
                }
                // Restart-required keys are recorded but not silently applied as live.
                "runtime.threads" | "log.format" | "entrypoints" => {
                    let mut pending = self.node.effective.pending_restart();
                    if !pending.iter().any(|p| p == k) {
                        pending.push(k.clone());
                    }
                    self.node.effective.set_pending_restart(pending);
                    applied.push(format!("{k} (pending_restart)"));
                }
                other => {
                    return Err(ConsoleError::Invalid(format!(
                        "unknown or unsupported cluster setting '{other}'"
                    )));
                }
            }
        }
        self.node.config.store(Arc::new(next));
        Ok(applied)
    }

    fn apply_namespace_settings(
        &self,
        ns: &str,
        settings: &Value,
    ) -> Result<Vec<String>, ConsoleError> {
        let obj = settings
            .as_object()
            .ok_or_else(|| ConsoleError::Invalid("settings must be an object".into()))?;
        // 006/007 namespace surface: persist labels / documented knobs on this node view.
        let mut store = self.ns_settings.lock();
        let mut current = store.get(ns).cloned().unwrap_or_else(|| json!({}));
        if !current.is_object() {
            current = json!({});
        }
        let map = current.as_object_mut().unwrap();
        let mut applied = Vec::new();
        for (k, v) in obj {
            match k.as_str() {
                "labels" => {
                    map.insert("labels".into(), v.clone());
                    applied.push("labels".into());
                }
                "replication" | "query_defaults" | "limits" => {
                    map.insert(k.clone(), v.clone());
                    applied.push(k.clone());
                }
                other => {
                    // Allow opaque namespace settings map (006/007 extend later).
                    map.insert(other.to_string(), v.clone());
                    applied.push(other.to_string());
                }
            }
        }
        store.insert(ns.to_string(), current);
        Ok(applied)
    }
}

impl ConsoleBackend for CatalogConsoleBackend {
    fn query(
        &self,
        authz: &ConsoleAuthz,
        req: &ConsoleQueryRequest,
    ) -> Result<Value, ConsoleError> {
        let ns = req.namespace.as_deref().unwrap_or("");
        let container = req.container.as_deref().unwrap_or("");
        let limit = req.limit.unwrap_or(100).min(MAX_PAGE_ROWS);
        let offset = req.offset.unwrap_or(0);
        let engine = self.engine();
        let result = if let Some(sql) = req.sql.as_deref().filter(|s| !s.trim().is_empty()) {
            engine
                .execute_blocking(
                    ns,
                    APPLICATION_UI,
                    APPLICATION_UI,
                    LogicalRequest::AdHocSql {
                        sql: sql.to_string(),
                    },
                    self.query_options(authz),
                    CancelToken::new(),
                )
                .map_err(|e| ConsoleError::Invalid(e.to_string()))?
        } else {
            engine
                .execute_blocking(
                    ns,
                    APPLICATION_UI,
                    APPLICATION_UI,
                    LogicalRequest::Scan {
                        container: container.to_string(),
                        filter: None,
                    },
                    self.query_options(authz),
                    CancelToken::new(),
                )
                .map_err(|e| ConsoleError::Invalid(e.to_string()))?
        };
        let mut rows: Vec<Value> = result
            .rows
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|cols| {
                let key = cols.first().and_then(|c| c.clone()).unwrap_or_default();
                let value = if cols.len() > 1 {
                    cols[1]
                        .as_ref()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_else(|| json!(cols[1]))
                } else {
                    Value::Null
                };
                json!({ "key": key, "value": value })
            })
            .collect();
        while serde_json::to_vec(&rows).map(|b| b.len()).unwrap_or(0) > MAX_PAGE_BYTES {
            if rows.is_empty() {
                return Err(ConsoleError::PayloadTooLarge);
            }
            rows.pop();
        }
        Ok(json!({
            "application": APPLICATION_UI,
            "rows": rows,
            "offset": offset,
            "limit": limit,
            "engine": "planner",
            "tag": result.tag,
        }))
    }

    fn config(
        &self,
        authz: &ConsoleAuthz,
        req: &ConsoleConfigRequest,
    ) -> Result<Value, ConsoleError> {
        match req.scope.as_str() {
            "cluster" => {
                if !authz.cluster_admin {
                    return Err(ConsoleError::Forbidden);
                }
                let applied = if req.settings.is_null()
                    || req
                        .settings
                        .as_object()
                        .map(|o| o.is_empty())
                        .unwrap_or(false)
                {
                    Vec::new()
                } else {
                    self.apply_cluster_settings(&req.settings)?
                };
                Ok(json!({
                    "ok": true,
                    "scope": "cluster",
                    "applied": applied,
                    "settings": self.cluster_settings_view(),
                }))
            }
            "namespace" => {
                let ns = req
                    .namespace
                    .as_deref()
                    .ok_or_else(|| ConsoleError::Invalid("namespace required".into()))?;
                if !authz.cluster_admin && !authz.namespace_admin.iter().any(|n| n == ns) {
                    return Err(ConsoleError::Forbidden);
                }
                let applied = if req.settings.is_null()
                    || req
                        .settings
                        .as_object()
                        .map(|o| o.is_empty())
                        .unwrap_or(false)
                {
                    Vec::new()
                } else {
                    self.apply_namespace_settings(ns, &req.settings)?
                };
                let stored = self
                    .ns_settings
                    .lock()
                    .get(ns)
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                Ok(json!({
                    "ok": true,
                    "scope": "namespace",
                    "namespace": ns,
                    "applied": applied,
                    "settings": stored,
                }))
            }
            _ => Err(ConsoleError::Invalid(
                "scope must be cluster|namespace".into(),
            )),
        }
    }

    fn create_container(
        &self,
        authz: &ConsoleAuthz,
        req: &ConsoleCreateContainer,
    ) -> Result<Value, ConsoleError> {
        let model = Self::model_for_type(&req.type_name);
        self.engine()
            .execute_blocking(
                &req.namespace,
                APPLICATION_UI,
                APPLICATION_UI,
                LogicalRequest::TypeOp {
                    container: req.name.clone(),
                    op: format!("ensure_{model}"),
                },
                self.query_options(authz),
                CancelToken::new(),
            )
            .map_err(|e| ConsoleError::Invalid(e.to_string()))?;
        Ok(json!({
            "namespace": req.namespace,
            "name": req.name,
            "type": req.type_name,
            "application": APPLICATION_UI,
            "engine": "planner",
        }))
    }
}
