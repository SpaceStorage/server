//! Audit entry shape — never includes key material (014 T014 stub).

use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use uuid::Uuid;

use crate::principal::PrincipalId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Uuid,
    pub principal_id: Option<PrincipalId>,
    pub login_at_event: Option<String>,
    pub action: String,
    pub target: String,
    pub namespace_id: Option<Uuid>,
    /// Opaque HLC stamp bytes until clocks crate lands.
    pub time_hlc: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct AuditListParams {
    pub limit: usize,
    pub cursor: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Default)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, entry: AuditEntry) {
        self.entries.push(entry);
    }

    pub fn list(&self, params: &AuditListParams) -> (Vec<serde_json::Value>, Option<String>) {
        let mut rows: Vec<_> = self.entries.iter().collect();
        rows.sort_by(|a, b| b.id.cmp(&a.id));
        let start = params
            .cursor
            .as_ref()
            .and_then(|c| Uuid::parse_str(c).ok())
            .map(|id| {
                rows.iter()
                    .position(|e| e.id == id)
                    .map(|i| i + 1)
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        let slice = rows.into_iter().skip(start).take(params.limit);
        let mut out = Vec::new();
        let mut last_id = None;
        for e in slice {
            last_id = Some(e.id);
            out.push(serde_json::json!({
                "id": e.id,
                "principal": e.login_at_event.as_deref().unwrap_or(""),
                "action": e.action,
                "target": e.target,
                "time": serde_json::Value::Null,
                "namespace": serde_json::Value::Null,
            }));
        }
        let next = if out.len() == params.limit {
            last_id.map(|id| id.to_string())
        } else {
            None
        };
        (out, next)
    }
}

#[derive(Debug, Default)]
pub struct SharedAuditLog(RwLock<AuditLog>);

impl SharedAuditLog {
    pub fn new() -> Self {
        Self(RwLock::new(AuditLog::new()))
    }

    pub fn append(&self, entry: AuditEntry) {
        if let Ok(mut g) = self.0.write() {
            g.append(entry);
        }
    }

    pub fn list(&self, params: AuditListParams) -> (Vec<serde_json::Value>, Option<String>) {
        match self.0.read() {
            Ok(g) => g.list(&params),
            Err(_) => (Vec::new(), None),
        }
    }
}