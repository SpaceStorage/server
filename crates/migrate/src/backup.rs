//! Backup / restore job orchestration over 013 SnapshotApi (010 US4).

use crate::authz::AuthzContext;
use crate::error::MigrateError;
use crate::job::{JobKind, JobRecord, Strategy};
use crate::store::JobStore;
use serde::{Deserialize, Serialize};
use spacestorage_backup::{
    ContainerSnapshotMeta, PitrTarget, RestoreRequest, RestoreService, SnapshotCreateRequest,
    SnapshotService, scope_container, scope_namespace,
};
use spacestorage_storage::StorageEngine;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupScope {
    Namespace {
        name: String,
    },
    Container {
        namespace: String,
        container: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSpec {
    pub scope: BackupScope,
    #[serde(default)]
    pub pitr: bool,
    #[serde(default)]
    pub containers: Vec<ContainerSnapshotMeta>,
    #[serde(default)]
    pub available_key_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreSpec {
    pub snapshot_id: Uuid,
    #[serde(default)]
    pub confirm_drop: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest_namespace: Option<String>,
    #[serde(default)]
    pub key_refs: Vec<String>,
    /// Optional PITR stop position forwarded to `013` `RestoreRequest.pitr` (FR-006).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitr_position: Option<PitrPosition>,
}

/// Wire form for restore PITR (positions map or HLC stamp).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PitrPosition {
    /// Explicit per-drive LSN map.
    Positions(BTreeMap<String, u64>),
    /// Tagged `{ "kind": "positions", "positions": { ... } }` or HLC.
    Target(PitrTarget),
    /// Bare HLC / ingest stamp.
    Hlc(u64),
}

impl PitrPosition {
    pub fn into_pitr_target(self) -> PitrTarget {
        match self {
            Self::Target(t) => t,
            Self::Positions(positions) => PitrTarget::Positions { positions },
            Self::Hlc(hlc) => PitrTarget::Hlc { hlc },
        }
    }
}

pub struct BackupOrchestrator {
    pub store: JobStore,
    pub slice10: bool,
}

impl BackupOrchestrator {
    pub async fn run_backup(
        &self,
        mut job: JobRecord,
        spec: &BackupSpec,
        engine: &StorageEngine,
        data_dir: std::path::PathBuf,
        authz: &AuthzContext,
    ) -> Result<JobRecord, MigrateError> {
        if !self.slice10 {
            return Err(MigrateError::MigrateSlice10Required);
        }
        authz.allow_node_migrate()?;
        job.mark_running();
        job.kind = JobKind::DataBackup;

        let scope = match &spec.scope {
            BackupScope::Namespace { name } => scope_namespace(name),
            BackupScope::Container {
                namespace,
                container,
            } => scope_container(namespace, container),
        };

        let svc = SnapshotService::new(data_dir, true);
        let man = svc
            .create(
                engine,
                SnapshotCreateRequest {
                    data_dir: svc.data_dir.clone(),
                    scope,
                    containers: spec.containers.clone(),
                    available_key_refs: spec.available_key_refs.clone(),
                },
            )
            .await?;

        job.snapshot_id = Some(man.snapshot_id);
        if spec.pitr {
            // Position map recorded on job body for 010 (not a single LSN).
            job.body = serde_json::json!({
                "snapshot_id": man.snapshot_id,
                "positions": man.positions,
                "pitr": true,
            });
        }
        job.progress.objects_copied = man.containers.len() as u64;
        job.mark_completed();
        job.strategy = Strategy::Snapshot;
        self.store.insert(job.clone());
        Ok(job)
    }

    pub async fn run_restore(
        &self,
        mut job: JobRecord,
        spec: &RestoreSpec,
        engine: &StorageEngine,
        data_dir: std::path::PathBuf,
        authz: &AuthzContext,
    ) -> Result<JobRecord, MigrateError> {
        if !self.slice10 {
            return Err(MigrateError::MigrateSlice10Required);
        }
        if spec.dest_namespace.is_some() {
            authz.allow_cross_namespace()?;
        } else {
            authz.allow_node_migrate()?;
        }
        job.mark_running();
        job.kind = JobKind::DataRestore;

        let mut key_refs = spec.key_refs.clone();
        if let Some(k) = &spec.key_ref {
            key_refs.push(k.clone());
        }

        let pitr = spec
            .pitr_position
            .clone()
            .map(PitrPosition::into_pitr_target);

        let restore = RestoreService::new(data_dir, true);
        let man = restore
            .restore_fill(
                engine,
                RestoreRequest {
                    snapshot_id: spec.snapshot_id,
                    confirm_drop: spec.confirm_drop,
                    pitr,
                    key_refs,
                    wal_stamps: Default::default(),
                },
            )
            .await?;

        job.snapshot_id = Some(man.snapshot_id);
        job.progress.objects_copied = man.containers.len() as u64;
        job.mark_completed();
        self.store.insert(job.clone());
        Ok(job)
    }
}
