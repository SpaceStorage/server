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
pub use backup::{BackupOrchestrator, BackupScope, BackupSpec, PitrPosition, RestoreSpec};
pub use dual_write::{
    CutoverGates, DualWriteRegistry, DualWriteRewrite, DualWriteWindow, apply_mapped,
    incomplete_name,
};
pub use error::MigrateError;
pub use job::{JobId, JobKind, JobProgress, JobRecord, JobStatus, Strategy};
pub use mapping::{
    CatalogMapping, MappingQuery, MappingSource, MappingSpec, apply_row_transform, catalog_default,
    execute_mapping_query, lower_to_logical_request, lower_to_logical_request_json,
    resolve_mapping,
};
pub use migrate::{
    ContainerRef, MigratePolicy, MigrateRunner, MigrationSpec, PlacementCheck, erase_with_legal,
};
pub use quota::{
    QuotaView, admit_dest_bytes, measure_row_bytes, recheck_dest_bytes, refresh_quota_view,
};
pub use store::{JobPersistHook, JobStore};
pub use transform::{RewriteKind, TransformRunner, TransformSpec};

use parking_lot::Mutex;
use serde_json::Value;
use spacestorage_observability::{LabelSet, help_for};
use spacestorage_tenancy::Tenancy;
use std::sync::Arc;
use uuid::Uuid;

/// Invoked after a job is accepted (`start` / `resume`) so the node can drive runners
/// against live content. Sync: the node typically spawns a tokio task inside.
pub type JobDriveHook = Arc<dyn Fn(JobId) + Send + Sync>;

/// Central job service: start / cancel / status / list / resume.
pub struct JobService {
    pub store: JobStore,
    pub dual: DualWriteRegistry,
    /// When false, all creates refuse with MigrateSlice10Required.
    pub slice10_enabled: bool,
    /// Destination namespace quotas (`07`) for migrate/transform admit.
    pub tenancy: Arc<Tenancy>,
    metrics: Option<Arc<spacestorage_observability::Metrics>>,
    cancel_requested: Mutex<std::collections::HashSet<JobId>>,
    drive_hook: Mutex<Option<JobDriveHook>>,
}

impl JobService {
    pub fn new(slice10_enabled: bool) -> Self {
        Self {
            store: JobStore::new(),
            dual: DualWriteRegistry::new(),
            slice10_enabled,
            tenancy: Arc::new(Tenancy::new()),
            metrics: None,
            cancel_requested: Mutex::new(std::collections::HashSet::new()),
            drive_hook: Mutex::new(None),
        }
    }

    pub fn with_metrics(mut self, metrics: Arc<spacestorage_observability::Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub fn with_tenancy(mut self, tenancy: Arc<Tenancy>) -> Self {
        self.tenancy = tenancy;
        self
    }

    /// Install the node supervisor that runs MigrateRunner / TransformRunner / BackupOrchestrator.
    pub fn set_drive_hook(&self, hook: Option<JobDriveHook>) {
        *self.drive_hook.lock() = hook;
    }

    pub fn set_persist_hook(&self, hook: Option<JobPersistHook>) {
        self.store.set_persist_hook(hook);
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

    fn request_drive(&self, id: JobId) {
        if let Some(h) = self.drive_hook.lock().as_ref() {
            h(id);
        }
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
                let spec: MigrationSpec = serde_json::from_value(body.clone())
                    .map_err(|e| MigrateError::Io(e.to_string()))?;
                spec.validate(authz)?;
            }
            JobKind::DataTransformation => {
                let spec: TransformSpec = serde_json::from_value(body.clone())
                    .map_err(|e| MigrateError::Io(e.to_string()))?;
                spec.validate(authz)?;
            }
            JobKind::DataBackup | JobKind::DataRestore => {
                authz.allow_node_migrate()?;
            }
        }
        let mut job = JobRecord::new(kind, principal_id, strategy, body);
        job.mark_running();
        // T074 / FR-008: refuse the job when `006` append / durable flush fails.
        self.store
            .insert_durable(job.clone())
            .map_err(|e| match e {
                MigrateError::Io(_) => MigrateError::StateUnreadable,
                other => other,
            })?;
        self.request_drive(job.id);
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
        let job = self
            .store
            .update(id, |j| {
                if j.progress.last_applied_source_seq.is_none() && j.paused {
                    j.mark_failed(&MigrateError::StateUnreadable);
                    return;
                }
                if j.status == JobStatus::Failed {
                    // Do not silently resurrect failed.
                    return;
                }
                j.set_paused(false);
                j.status = JobStatus::Running;
            })
            .map_err(|e| match e {
                MigrateError::Io(_) => MigrateError::StateUnreadable,
                other => other,
            })?;
        if job.status == JobStatus::Failed
            && job.error_code.as_deref() == Some(MigrateError::StateUnreadable.code())
        {
            return Err(MigrateError::StateUnreadable);
        }
        if job.status == JobStatus::Running && !job.paused {
            self.request_drive(job.id);
        }
        Ok(job)
    }

    pub fn complete(&self, id: JobId) -> Result<JobRecord, MigrateError> {
        let was = self.store.get(id).map(|j| j.status);
        let job = self.store.update(id, |j| j.mark_completed())?;
        if was != Some(JobStatus::Completed) {
            self.bump_job_metric(job.kind, "completed");
        }
        Ok(job)
    }

    pub fn fail(&self, id: JobId, err: &MigrateError) -> Result<JobRecord, MigrateError> {
        let was = self.store.get(id).map(|j| j.status);
        let job = self.store.update(id, |j| j.mark_failed(err))?;
        if was != Some(JobStatus::Failed) {
            self.bump_job_metric(job.kind, "failed");
        }
        Ok(job)
    }

    /// Pause a running job after dual-write target failure (status stays `running`).
    pub fn pause_with_error(
        &self,
        id: JobId,
        err: &MigrateError,
    ) -> Result<JobRecord, MigrateError> {
        self.store.update(id, |j| {
            if j.status == JobStatus::Running {
                j.set_paused(true);
                j.error_code = Some(err.code().into());
                j.error = Some(err.to_string());
            }
        })
    }

    pub fn migrate_runner(&self) -> MigrateRunner {
        MigrateRunner {
            store: self.store.clone(),
            dual: self.dual.clone(),
            slice10: self.slice10_enabled,
            placement: PlacementCheck::default(),
            tenancy: Arc::clone(&self.tenancy),
        }
    }

    /// Migrate runner with `04` placement inputs (node supplies membership topology).
    pub fn migrate_runner_with_placement(&self, placement: PlacementCheck) -> MigrateRunner {
        MigrateRunner {
            store: self.store.clone(),
            dual: self.dual.clone(),
            slice10: self.slice10_enabled,
            placement,
            tenancy: Arc::clone(&self.tenancy),
        }
    }

    pub fn transform_runner(&self) -> TransformRunner {
        TransformRunner {
            store: self.store.clone(),
            dual: self.dual.clone(),
            slice10: self.slice10_enabled,
            tenancy: Arc::clone(&self.tenancy),
        }
    }

    pub fn backup_orchestrator(&self) -> BackupOrchestrator {
        BackupOrchestrator {
            store: self.store.clone(),
            slice10: self.slice10_enabled,
        }
    }

    pub fn is_cancel_requested(&self, id: JobId) -> bool {
        self.cancel_requested.lock().contains(&id)
    }
}

/// First-binary stub: always refuses.
pub fn stub_service() -> JobService {
    JobService::new(false)
}
