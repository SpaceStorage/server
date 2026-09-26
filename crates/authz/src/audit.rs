//! Audit entry shape — never includes key material (014 T014 stub).

use serde::{Deserialize, Serialize};
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
