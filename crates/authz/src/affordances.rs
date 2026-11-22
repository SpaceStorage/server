//! UI affordance flags from closed 014 verbs (data-model.md).

use crate::session::SessionToken;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct Affordances {
    pub namespace_write: bool,
    pub quota_replace: bool,
    pub config_reload: bool,
    pub cluster_stop: bool,
    pub node_drain: bool,
    pub job_control: bool,
    pub audit_read: bool,
    pub metrics_read: bool,
    pub map: bool,
    pub browse: bool,
}

pub fn affordances_for_session(rec: &SessionToken) -> Affordances {
    if rec.cluster_admin {
        return Affordances {
            namespace_write: true,
            quota_replace: true,
            config_reload: true,
            cluster_stop: true,
            node_drain: true,
            job_control: true,
            audit_read: true,
            metrics_read: true,
            map: true,
            browse: true,
        };
    }
    if !rec.namespace_scopes.is_empty() {
        return Affordances {
            map: true,
            browse: true,
            metrics_read: true,
            ..Affordances::default()
        };
    }
    Affordances::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionToken;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    fn ns_session() -> SessionToken {
        SessionToken {
            token_hash: [0; 32],
            principal_id: Uuid::new_v4(),
            generation: 0,
            login: "ns".into(),
            cluster_admin: false,
            csrf_token: "aa".into(),
            namespace_scopes: vec!["acme".into()],
            expires_at: Instant::now() + Duration::from_secs(3600),
        }
    }

    #[test]
    fn namespace_session_has_no_cluster_write_affordances() {
        let aff = affordances_for_session(&ns_session());
        assert!(!aff.namespace_write);
        assert!(!aff.quota_replace);
        assert!(!aff.config_reload);
        assert!(!aff.cluster_stop);
        assert!(!aff.node_drain);
        assert!(!aff.job_control);
    }
}