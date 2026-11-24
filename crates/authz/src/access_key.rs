//! SigV4 access keys bound to one namespace (014 store, consumed by 019).

use std::collections::HashMap;

use parking_lot::RwLock;

use crate::permission::Verb;

#[derive(Debug, Clone)]
pub struct AccessKeyRecord {
    pub access_key_id: String,
    pub secret: String,
    /// `None` is an unbound principal. S3 refuses it before any data op (FR-006).
    pub namespace: Option<String>,
    /// When set, `X-Amz-Security-Token` (header or presign query) must match.
    pub session_token: Option<String>,
    pub grants: Verb,
    pub login: String,
}

/// In-process access-key registry. Cluster replication of key material is a later seam;
/// S3 resolves secrets and namespace binding through this store, not a hard-coded demo key.
#[derive(Debug, Default)]
pub struct AccessKeyStore {
    by_id: RwLock<HashMap<String, AccessKeyRecord>>,
}

impl AccessKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, record: AccessKeyRecord) {
        self.by_id
            .write()
            .insert(record.access_key_id.clone(), record);
    }

    pub fn lookup(&self, access_key_id: &str) -> Option<AccessKeyRecord> {
        self.by_id.read().get(access_key_id).cloned()
    }

    pub fn issue(
        &self,
        access_key_id: impl Into<String>,
        secret: impl Into<String>,
        namespace: Option<String>,
        session_token: Option<String>,
        grants: Verb,
        login: impl Into<String>,
    ) -> AccessKeyRecord {
        let record = AccessKeyRecord {
            access_key_id: access_key_id.into(),
            secret: secret.into(),
            namespace,
            session_token,
            grants,
            login: login.into(),
        };
        self.insert(record.clone());
        record
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_roundtrip_and_missing() {
        let store = AccessKeyStore::new();
        store.issue(
            "AKIA",
            "secret",
            Some("ns1".into()),
            Some("tok".into()),
            Verb::READ,
            "alice",
        );
        let got = store.lookup("AKIA").expect("present");
        assert_eq!(got.namespace.as_deref(), Some("ns1"));
        assert_eq!(got.session_token.as_deref(), Some("tok"));
        assert!(got.grants.contains(Verb::READ));
        assert!(store.lookup("missing").is_none());
    }
}
