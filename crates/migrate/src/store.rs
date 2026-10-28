//! Job store (010). In-memory index + optional durable sink (006 cluster/namespace log).

use crate::error::MigrateError;
use crate::job::{JobId, JobRecord};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Durable append of a job record. Node wires this to `006` ClusterStore (cluster log when
/// namespaces/placement/backup scope differ; namespace log otherwise) or a JSONL fallback.
pub type JobPersistHook = Arc<dyn Fn(&JobRecord) -> Result<(), String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct JobStore {
    inner: Arc<RwLock<HashMap<JobId, JobRecord>>>,
    persist: Arc<RwLock<Option<JobPersistHook>>>,
}

impl JobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_persist_hook(&self, hook: Option<JobPersistHook>) {
        *self.persist.write() = hook;
    }

    fn flush(&self, job: &JobRecord) -> Result<(), MigrateError> {
        if let Some(hook) = self.persist.read().as_ref() {
            hook(job).map_err(MigrateError::Io)?;
        }
        Ok(())
    }

    /// Best-effort insert for in-flight runner progress (flush errors ignored).
    /// Prefer [`Self::insert_durable`] for `start` / `resume` acceptance (FR-008 / T074).
    pub fn insert(&self, job: JobRecord) -> JobId {
        let id = job.id;
        let _ = self.flush(&job);
        self.inner.write().insert(id, job);
        id
    }

    /// Insert and require durable flush (`006` / JSONL). Failures must refuse the job.
    pub fn insert_durable(&self, job: JobRecord) -> Result<JobId, MigrateError> {
        let id = job.id;
        self.flush(&job)?;
        self.inner.write().insert(id, job);
        Ok(id)
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
        let out = job.clone();
        drop(map);
        self.flush(&out)?;
        // Re-read in case flush is best-effort; keep memory copy.
        self.inner.write().insert(id, out.clone());
        Ok(out)
    }

    pub fn list(&self) -> Vec<JobRecord> {
        let mut v: Vec<_> = self.inner.read().values().cloned().collect();
        v.sort_by_key(|j| j.id);
        v
    }

    /// Hydrate from durable snapshot (cluster/namespace log replay).
    pub fn load_all(&self, jobs: impl IntoIterator<Item = JobRecord>) {
        let mut map = self.inner.write();
        for j in jobs {
            map.insert(j.id, j);
        }
    }

    /// Whether a job should prefer the cluster log (cross-ns, placement, backup/restore).
    pub fn prefers_cluster_log(job: &JobRecord) -> bool {
        if matches!(
            job.kind,
            crate::job::JobKind::DataBackup | crate::job::JobKind::DataRestore
        ) {
            return true;
        }
        let body = &job.body;
        let cross_ns = body
            .get("dest_namespace")
            .and_then(|v| v.as_str())
            .zip(
                body.get("source")
                    .and_then(|s| s.get("namespace"))
                    .and_then(|n| n.as_str()),
            )
            .is_some_and(|(d, s)| d != s);
        let placement = body
            .get("dest_nodes")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty())
            || body
                .get("dest_drives")
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty());
        cross_ns || placement
    }
}
