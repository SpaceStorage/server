//! MigrationSpec + live/snapshot/offline runners (010 US1/US2).

use crate::authz::AuthzContext;
use crate::dual_write::{incomplete_name, apply_mapped, CutoverGates, DualWriteRegistry, DualWriteWindow};
use crate::error::MigrateError;
use crate::job::{JobKind, JobRecord, JobStatus, Strategy};
use crate::quota::QuotaView;
use crate::store::JobStore;
use serde::{Deserialize, Serialize};
use spacestorage_storage::{ContentStore, StorageMode};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerRef {
    pub namespace: String,
    pub container: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MigratePolicy {
    #[default]
    Copy,
    Move,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationSpec {
    pub source: ContainerRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest_namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest_name: Option<String>,
    #[serde(default)]
    pub policy: MigratePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest_nodes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest_drives: Option<Vec<String>>,
    #[serde(default)]
    pub strategy: Strategy,
    /// When true, placement director would refuse (injected for tests / seam).
    #[serde(default)]
    pub force_unsatisfiable: bool,
}

impl MigrationSpec {
    pub fn validate(&self, authz: &AuthzContext) -> Result<(), MigrateError> {
        let cross_ns = self
            .dest_namespace
            .as_ref()
            .is_some_and(|ns| ns != &self.source.namespace);
        if cross_ns {
            authz.allow_cross_namespace()?;
        } else {
            authz.allow_node_migrate()?;
        }

        let has_dest_placement = self
            .dest_nodes
            .as_ref()
            .is_some_and(|n| !n.is_empty())
            || self.dest_drives.as_ref().is_some_and(|d| !d.is_empty());

        if !cross_ns && !has_dest_placement && matches!(self.policy, MigratePolicy::Copy) {
            return Err(MigrateError::NoOp);
        }
        if self.force_unsatisfiable {
            return Err(MigrateError::ConstraintUnsatisfiable);
        }
        Ok(())
    }

    pub fn dest_public_name(&self) -> String {
        self.dest_name
            .clone()
            .unwrap_or_else(|| self.source.container.clone())
    }

    pub fn dest_ns(&self) -> &str {
        self.dest_namespace
            .as_deref()
            .unwrap_or(self.source.namespace.as_str())
    }
}

/// In-memory migrate runner for loopback / conformance (dual-write + catch-up).
pub struct MigrateRunner {
    pub store: JobStore,
    pub dual: DualWriteRegistry,
    pub slice10: bool,
}

impl MigrateRunner {
    pub fn run_live(
        &self,
        mut job: JobRecord,
        spec: &MigrationSpec,
        source_id: Uuid,
        source_rows: &HashMap<Vec<u8>, Vec<u8>>,
        source_head: u64,
        dest_names: &mut HashMap<(String, String), Uuid>,
        content: &mut ContentStore,
        quota: QuotaView,
        source_bytes: u64,
    ) -> Result<JobRecord, MigrateError> {
        if !self.slice10 {
            return Err(MigrateError::MigrateSlice10Required);
        }
        quota.check_fits(source_bytes)?;

        let dest_key = (spec.dest_ns().to_string(), spec.dest_public_name());
        if dest_names.contains_key(&dest_key) {
            return Err(MigrateError::NameExists {
                container: format!("{}.{}", dest_key.0, dest_key.1),
            });
        }

        let target_id = Uuid::now_v7();
        let internal = incomplete_name(job.id);
        job.target_internal_name = Some(internal.clone());
        content.register_mode(target_id, StorageMode::Persistent);

        let install_seq = source_head;
        self.dual.register(DualWriteWindow {
            container_id: source_id,
            job_id: job.id,
            install_seq,
            target_id,
        })?;

        job.mark_running();
        job.progress.install_seq = Some(install_seq);
        job.progress.last_source_seq = Some(source_head);

        // Catch-up: copy all source rows as seq 0..=install_seq (idempotent).
        let mut applied = HashMap::new();
        let mut dest_map = HashMap::new();
        let mut objects = 0u64;
        let mut bytes = 0u64;
        for (k, v) in source_rows {
            if apply_mapped(
                &mut applied,
                k.clone(),
                0,
                v.clone(),
                &mut dest_map,
            ) {
                objects += 1;
                bytes += (k.len() + v.len()) as u64;
            }
        }
        for (k, v) in dest_map {
            content.put(target_id, k, v);
        }
        job.progress.objects_copied = objects;
        job.progress.bytes_copied = bytes;
        job.progress.last_applied_source_seq = Some(install_seq);

        if matches!(spec.strategy, Strategy::Offline) {
            // Offline: writes already refused by caller via paused_writes flag.
        }

        let gates = CutoverGates {
            last_applied_source_seq: install_seq,
            source_head_seq: source_head,
            quota,
            additional_bytes: source_bytes,
            placement_ok: !spec.force_unsatisfiable,
            target_complete: true,
            dest_name_free_or_swap: !dest_names.contains_key(&dest_key),
        };
        if let Err(e) = gates.check() {
            job.mark_failed(&e);
            self.dual.unregister(source_id);
            self.store.insert(job.clone());
            return Err(e);
        }

        // Publish dest name; drop source on move.
        dest_names.insert(dest_key, target_id);
        if matches!(spec.policy, MigratePolicy::Move) {
            // Drop source: clear content + remove name mapping.
            content.clear_content(source_id);
        }
        self.dual.unregister(source_id);
        job.mark_completed();
        self.store.insert(job.clone());
        Ok(job)
    }

    pub fn cancel(&self, id: Uuid) -> Result<JobRecord, MigrateError> {
        self.store.update(id, |j| {
            if matches!(j.status, JobStatus::Completed | JobStatus::Failed) {
                return;
            }
            // Leave source intact; target stays incomplete (never swapped).
            j.mark_failed(&MigrateError::Cancelled);
        })
    }
}

pub fn parse_migration_body(v: &serde_json::Value) -> Result<MigrationSpec, MigrateError> {
    serde_json::from_value(v.clone()).map_err(|e| MigrateError::Io(e.to_string()))
}

pub fn kind_for_migration() -> JobKind {
    JobKind::DataMigration
}
