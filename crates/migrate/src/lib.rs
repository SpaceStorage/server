//! Data migration, transforms, and backup/restore jobs (010).

pub mod authz;
pub mod backup;
pub mod dual_write;
pub mod error;
pub mod job;
pub mod mapping;
pub mod migrate;
pub mod quota;
pub mod store;
pub mod transform;

pub use authz::AuthzContext;
pub use backup::{BackupOrchestrator, BackupScope, BackupSpec, RestoreSpec};
pub use dual_write::{
    apply_mapped, incomplete_name, CutoverGates, DualWriteRegistry, DualWriteWindow,
};
pub use error::MigrateError;
pub use job::{JobId, JobKind, JobProgress, JobRecord, JobStatus, Strategy};
pub use mapping::{
    catalog_default, lower_to_logical_request, resolve_mapping, CatalogMapping, MappingQuery,
    MappingSource, MappingSpec,
};
pub use migrate::{
    erase_with_legal, ContainerRef, MigratePolicy, MigrateRunner, MigrationSpec,
};
pub use quota::QuotaView;
pub use store::JobStore;
pub use transform::{RewriteKind, TransformRunner, TransformSpec};

use parking_lot::Mutex;
use serde_json::Value;
use spacestorage_observability::{help_for, LabelSet};
use std::sync::Arc;
use uuid::Uuid;

/// Central job service: start / cancel / status / list / resume.
pub struct JobService {
    pub store: JobStore,
    pub dual: DualWriteRegistry,
    /// When false, all creates refuse with MigrateSlice10Required.
    pub slice10_enabled: bool,
    metrics: Option<Arc<spacestorage_observability::Metrics>>,
    cancel_requested: Mutex<std::collections::HashSet<JobId>>,
}

impl JobService {
    pub fn new(slice10_enabled: bool) -> Self {
        Self {
            store: JobStore::new(),
            dual: DualWriteRegistry::new(),
            slice10_enabled,
            metrics: None,
            cancel_requested: Mutex::new(std::collections::HashSet::new()),
        }
    }

    pub fn with_metrics(mut self, metrics: Arc<spacestorage_observability::Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    fn bump_job_metric(&self, kind: JobKind, status: &str) {
        let Some(m) = &self.metrics else {
            return;
        };
        let mut labels = LabelSet::new();
        let _ = labels.insert("job", kind.as_str());
        let _ = labels.insert("status", status);
        m.registry().inc(
            "spacestorage_job_queue_completed_total",
            labels,
            help_for("spacestorage_job_queue_completed_total"),
            1,
        );
    }

    pub fn start(
        &self,
        kind: JobKind,
        principal_id: Uuid,
        strategy: Strategy,
        body: Value,
        authz: &AuthzContext,
    ) -> Result<JobRecord, MigrateError> {
        if !self.slice10_enabled {
            return Err(MigrateError::MigrateSlice10Required);
        }
        match kind {
            JobKind::DataMigration => {
                let spec: MigrationSpec =
                    serde_json::from_value(body.clone()).map_err(|e| MigrateError::Io(e.to_string()))?;
                spec.validate(authz)?;
            }
            JobKind::DataTransformation => {
                let spec: TransformSpec =
                    serde_json::from_value(body.clone()).map_err(|e| MigrateError::Io(e.to_string()))?;
                spec.validate(authz)?;
            }
            JobKind::DataBackup | JobKind::DataRestore => {
                authz.allow_node_migrate()?;
            }
        }
        let mut job = JobRecord::new(kind, principal_id, strategy, body);
        job.mark_running();
        self.store.insert(job.clone());
        Ok(job)
    }

    pub fn status(&self, id: JobId) -> Result<JobRecord, MigrateError> {
        self.store.get(id).ok_or(MigrateError::NotFound)
    }

    pub fn list(&self) -> Vec<JobRecord> {
        self.store.list()
    }

    pub fn cancel(&self, id: JobId) -> Result<JobRecord, MigrateError> {
        self.cancel_requested.lock().insert(id);
        let job = self.store.update(id, |j| {
            if matches!(j.status, JobStatus::Completed | JobStatus::Failed) {
                return;
            }
            j.mark_failed(&MigrateError::Cancelled);
        })?;
        self.bump_job_metric(job.kind, "failed");
        Ok(job)
    }

    pub fn resume(&self, id: JobId) -> Result<JobRecord, MigrateError> {
        self.store.update(id, |j| {
            if j.status != JobStatus::Running && !j.paused {
                // Allow resume from paused running or re-drive after restart while running.
            }
            if j.progress.last_applied_source_seq.is_none() && j.paused {
                j.mark_failed(&MigrateError::StateUnreadable);
                return;
            }
            j.set_paused(false);
            if j.status == JobStatus::Failed {
                // Do not silently resurrect failed.
            } else {
                j.status = JobStatus::Running;
            }
        })
    }

    pub fn complete(&self, id: JobId) -> Result<JobRecord, MigrateError> {
        let job = self.store.update(id, |j| j.mark_completed())?;
        self.bump_job_metric(job.kind, "completed");
        Ok(job)
    }

    pub fn fail(&self, id: JobId, err: &MigrateError) -> Result<JobRecord, MigrateError> {
        let job = self.store.update(id, |j| j.mark_failed(err))?;
        self.bump_job_metric(job.kind, "failed");
        Ok(job)
    }

    pub fn migrate_runner(&self) -> MigrateRunner {
        MigrateRunner {
            store: self.store.clone(),
            dual: self.dual.clone(),
            slice10: self.slice10_enabled,
        }
    }

    pub fn transform_runner(&self) -> TransformRunner {
        TransformRunner {
            store: self.store.clone(),
            dual: self.dual.clone(),
            slice10: self.slice10_enabled,
        }
    }

    pub fn backup_orchestrator(&self) -> BackupOrchestrator {
        BackupOrchestrator {
            store: self.store.clone(),
            slice10: self.slice10_enabled,
        }
    }
}

/// First-binary stub: always refuses.
pub fn stub_service() -> JobService {
    JobService::new(false)
}
