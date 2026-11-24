//! Namespace-bound SigV4 credentials backed by the 014 access-key store (019 T014/T015).

use std::sync::Arc;

use spacestorage_authz::{AccessKeyRecord, AccessKeyStore, Verb};

use crate::sigv4::{DEMO_ACCESS_KEY, DEMO_SECRET_KEY};

#[derive(Debug, Clone)]
pub struct S3Credential {
    pub secret: String,
    /// `None` = unbound principal (FR-006); refused on data ops and ListBuckets.
    pub namespace: Option<String>,
    /// When set, requests MUST include matching `X-Amz-Security-Token`.
    pub session_token: Option<String>,
    pub grants: u16,
    pub login: String,
}

/// Lookup facade over [`AccessKeyStore`]. Production SigV4 resolves secrets here.
#[derive(Debug, Clone)]
pub struct CredentialStore {
    keys: Arc<AccessKeyStore>,
}

impl CredentialStore {
    pub fn from_store(keys: Arc<AccessKeyStore>) -> Arc<Self> {
        Arc::new(Self { keys })
    }

    pub fn with_demo_default() -> Arc<Self> {
        let keys = Arc::new(AccessKeyStore::new());
        keys.issue(
            DEMO_ACCESS_KEY,
            DEMO_SECRET_KEY,
            Some("demo".into()),
            None,
            Verb::IMPLICIT_TENANT,
            "demo",
        );
        Self::from_store(keys)
    }

    pub fn store(&self) -> &AccessKeyStore {
        &self.keys
    }

    pub fn register(&self, access_key: &str, cred: S3Credential) {
        self.keys.insert(AccessKeyRecord {
            access_key_id: access_key.into(),
            secret: cred.secret,
            namespace: cred.namespace,
            session_token: cred.session_token,
            grants: Verb::from_bits(cred.grants),
            login: cred.login,
        });
    }

    pub fn lookup(&self, access_key: &str) -> Option<S3Credential> {
        self.keys.lookup(access_key).map(|rec| S3Credential {
            secret: rec.secret,
            namespace: rec.namespace,
            session_token: rec.session_token,
            grants: rec.grants.bits(),
            login: rec.login,
        })
    }

    pub fn secret_for(&self, access_key: &str) -> Option<String> {
        self.lookup(access_key).map(|c| c.secret)
    }
}

/// Validate optional STS-style session token for temporary credentials.
pub fn check_session_token(
    cred: &S3Credential,
    presented: Option<&str>,
) -> Result<(), (&'static str, &'static str)> {
    let presented = presented.map(str::trim).filter(|s| !s.is_empty());
    match (&cred.session_token, presented) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(("AccessDenied", "InvalidToken")),
        (Some(expected), Some(got)) if got == expected.as_str() => Ok(()),
        (Some(_), Some(_)) => Err(("AccessDenied", "InvalidToken")),
        (Some(_), None) => Err(("AccessDenied", "MissingToken")),
    }
}
