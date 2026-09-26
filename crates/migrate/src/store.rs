//! In-process job store (010). Cluster/namespace Raft append is a seam for 006.

use crate::error::MigrateError;
use crate::job::{JobId, JobRecord};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct JobStore {
    inner: Arc<RwLock<HashMap<JobId, JobRecord>>>,
}

impl JobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, job: JobRecord) -> JobId {
        let id = job.id;
        self.inner.write().insert(id, job);
        id
    }

    pub fn get(&self, id: JobId) -> Option<JobRecord> {
        self.inner.read().get(&id).cloned()
    }

    pub fn update<F>(&self, id: JobId, f: F) -> Result<JobRecord, MigrateError>
    where
        F: FnOnce(&mut JobRecord),
    {
        let mut map = self.inner.write();
        let job = map.get_mut(&id).ok_or(MigrateError::NotFound)?;
        f(job);
        Ok(job.clone())
    }

    pub fn list(&self) -> Vec<JobRecord> {
        let mut v: Vec<_> = self.inner.read().values().cloned().collect();
        v.sort_by_key(|j| j.id);
        v
    }
}
