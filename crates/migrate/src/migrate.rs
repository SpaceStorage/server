//! MigrationSpec + live/snapshot/offline runners (010 US1/US2).

use crate::authz::AuthzContext;
use crate::dual_write::{incomplete_name, apply_mapped, CutoverGates, DualWriteRegistry, DualWriteWindow};
use crate::error::MigrateError;
use crate::job::{JobKind, JobRecord, JobStatus, Strategy};
use crate::quota::{measure_row_bytes, refresh_quota_view, QuotaView};
use crate::store::JobStore;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use spacestorage_placement::topology::NodeTopo;
use spacestorage_placement::{plan_replicas, PlacementError, PlanRequest};
use spacestorage_storage::{ContentStore, EraseRecord, EraseRequest, LegalError, LegalHoldStore, StorageMode};
use spacestorage_tenancy::Tenancy;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Topology / RF inputs for `04` `plan_replicas` (FR-009). Empty `candidates` skips the
/// planner (in-memory unit paths); the node always supplies membership candidates.
#[derive(Debug, Clone, Default)]
pub struct PlacementCheck {
    pub candidates: Vec<NodeTopo>,
    pub topology_ladder: Vec<String>,
    pub replication_factor: u32,
    pub anti_affinity_keys: Vec<String>,
    /// Drive ids known to be available for `dest_drives` checks.
    pub available_drives: Vec<String>,
}

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
        // Test / inject seam only — real placement uses `validate_destination_placement`.
        if self.force_unsatisfiable {
            return Err(MigrateError::ConstraintUnsatisfiable);
        }
        Ok(())
    }

    /// Validate destination with `04` `plan_replicas` (and drive availability).
    /// Call before migrate start and again before cutover (FR-009).
    pub fn validate_destination_placement(
        &self,
        check: &PlacementCheck,
    ) -> Result<(), MigrateError> {
        if self.force_unsatisfiable {
            return Err(MigrateError::ConstraintUnsatisfiable);
        }
        if let Some(drives) = &self.dest_drives {
            if !drives.is_empty() {
                if check.available_drives.is_empty() {
                    // Node did not publish drive inventory — cannot satisfy named drives.
                    return Err(MigrateError::ConstraintUnsatisfiable);
                }
                for d in drives {
                    if !check.available_drives.iter().any(|a| a == d) {
                        return Err(MigrateError::ConstraintUnsatisfiable);
                    }
                }
            }
        }
        if check.candidates.is_empty() {
            // In-memory runners without topology: planner not applicable.
            return Ok(());
        }

        let mut candidates = check.candidates.clone();
        if let Some(nodes) = &self.dest_nodes {
            if !nodes.is_empty() {
                for n in nodes {
                    if !check.candidates.iter().any(|c| c.name == *n) {
                        return Err(MigrateError::ConstraintUnsatisfiable);
                    }
                }
                candidates.retain(|c| nodes.iter().any(|n| n == &c.name));
            }
        }

        let rf = self
            .dest_nodes
            .as_ref()
            .filter(|n| !n.is_empty())
            .map(|n| n.len() as u32)
            .unwrap_or_else(|| check.replication_factor.max(1));

        let ladder = if check.topology_ladder.is_empty() {
            vec!["az".into()]
        } else {
            check.topology_ladder.clone()
        };

        match plan_replicas(&PlanRequest {
            container: self.source.container.clone(),
            replication_factor: rf,
            anti_affinity_keys: check.anti_affinity_keys.clone(),
            topology_ladder: ladder,
            candidates,
        }) {
            Ok(_) => Ok(()),
            Err(PlacementError::PlacementUnsatisfiable { .. })
            | Err(PlacementError::AntiAffinityKeyMissing)
            | Err(PlacementError::QuorumUnsatisfiable) => {
                Err(MigrateError::ConstraintUnsatisfiable)
            }
            Err(e) => Err(MigrateError::Io(e.to_string())),
        }
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
    /// Optional `04` placement inputs; empty candidates skip planner (unit paths).
    pub placement: PlacementCheck,
    /// Destination namespace quotas (`07`) re-read immediately before cutover (T077).
    pub tenancy: Arc<Tenancy>,
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
        // Placement before start (FR-009).
        spec.validate_destination_placement(&self.placement)?;
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
            rewrite: None,
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
        // Drain live dual-write buffers (including puts acked after the first drain) — T076.
        let mut last_applied = install_seq;
        loop {
            let pending = self.dual.take_pending(target_id);
            if pending.is_empty() {
                break;
            }
            for (k, v) in pending {
                content.put(target_id, k.clone(), v.clone());
                objects += 1;
                bytes += (k.len() + v.len()) as u64;
                last_applied = last_applied.saturating_add(1);
            }
        }
        job.progress.objects_copied = objects;
        job.progress.bytes_copied = bytes;
        job.progress.last_applied_source_seq = Some(last_applied.max(install_seq));

        if matches!(spec.strategy, Strategy::Offline) {
            // Offline: writes already refused via DualWriteRegistry::pause_writes.
        }

        // T077: re-read `07` usage and grown source size immediately before cutover.
        let grown = self
            .dual
            .grown_source_bytes(source_id, source_bytes)
            .max(measure_row_bytes(source_rows));
        let quota = refresh_quota_view(&self.tenancy, spec.dest_ns(), quota);

        // Placement again before cutover (FR-009) — not `force_unsatisfiable` alone.
        let placement_ok = spec.validate_destination_placement(&self.placement).is_ok();
        let effective_head = source_head.max(last_applied);
        let gates = CutoverGates {
            last_applied_source_seq: last_applied.max(install_seq),
            source_head_seq: effective_head,
            quota,
            additional_bytes: grown,
            placement_ok,
            target_complete: true,
            dest_name_free_or_swap: !dest_names.contains_key(&dest_key),
        };
        if let Err(e) = gates.check() {
            job.mark_failed(&e);
            let _ = self.dual.unregister(source_id);
            self.store.insert(job.clone());
            return Err(e);
        }

        // Publish dest name; drop source on move (name reusable, storage released).
        dest_names.insert(dest_key, target_id);
        if matches!(spec.policy, MigratePolicy::Move) {
            content.clear_content(source_id);
            let src_key = (
                spec.source.namespace.clone(),
                spec.source.container.clone(),
            );
            dest_names.remove(&src_key);
        }
        // Final drain into content while the dual-write window stays registered.
        // T078: unregister only after catalog publish so a source ack cannot miss cutover.
        loop {
            let pending = self.dual.take_pending(target_id);
            if pending.is_empty() {
                break;
            }
            for (k, v) in pending {
                content.put(target_id, k.clone(), v.clone());
                objects += 1;
                bytes += (k.len() + v.len()) as u64;
            }
        }
        job.progress.objects_copied = objects;
        job.progress.bytes_copied = bytes;
        job.mark_completed();
        self.store.insert(job.clone());
        Ok(job)
    }

    /// Offline: refuse source writes for the window, then same catch-up/cutover as live.
    pub fn run_offline(
        &self,
        job: JobRecord,
        spec: &MigrationSpec,
        source_id: Uuid,
        source_rows: &HashMap<Vec<u8>, Vec<u8>>,
        source_head: u64,
        dest_names: &mut HashMap<(String, String), Uuid>,
        content: &mut ContentStore,
        quota: QuotaView,
        source_bytes: u64,
    ) -> Result<JobRecord, MigrateError> {
        self.dual.pause_writes(source_id);
        let mut spec = spec.clone();
        spec.strategy = Strategy::Offline;
        let result = self.run_live(
            job,
            &spec,
            source_id,
            source_rows,
            source_head,
            dest_names,
            content,
            quota,
            source_bytes,
        );
        self.dual.resume_writes(source_id);
        result
    }

    /// Snapshot strategy via `013`: snapshot the source, restore into the destination.
    /// No dual-write window (contracts/dual-write.md / R6).
    pub async fn run_snapshot(
        &self,
        mut job: JobRecord,
        spec: &MigrationSpec,
        source_id: Uuid,
        source_meta: spacestorage_backup::ContainerSnapshotMeta,
        dest_names: &mut HashMap<(String, String), Uuid>,
        engine: &spacestorage_storage::StorageEngine,
        data_dir: std::path::PathBuf,
        quota: QuotaView,
        source_bytes: u64,
    ) -> Result<JobRecord, MigrateError> {
        if !self.slice10 {
            return Err(MigrateError::MigrateSlice10Required);
        }
        if let Err(e) = spec.validate_destination_placement(&self.placement) {
            job.mark_failed(&e);
            self.store.insert(job.clone());
            return Err(e);
        }
        quota.check_fits(source_bytes)?;
        let dest_key = (spec.dest_ns().to_string(), spec.dest_public_name());
        if dest_names.contains_key(&dest_key) {
            return Err(MigrateError::NameExists {
                container: format!("{}.{}", dest_key.0, dest_key.1),
            });
        }

        job.target_internal_name = Some(incomplete_name(job.id));
        job.mark_running();
        job.strategy = Strategy::Snapshot;

        let snap = spacestorage_backup::SnapshotService::new(data_dir.clone(), true);
        let man = snap
            .create(
                engine,
                spacestorage_backup::SnapshotCreateRequest {
                    data_dir: snap.data_dir.clone(),
                    scope: spacestorage_backup::scope_container(
                        &spec.source.namespace,
                        &spec.source.container,
                    ),
                    containers: vec![source_meta.clone()],
                    available_key_refs: source_meta
                        .key_ref
                        .clone()
                        .into_iter()
                        .collect(),
                },
            )
            .await?;

        // Restore fills engine content for source container id; remap to a new
        // destination id so the public dest name does not collide with source.
        let restore = spacestorage_backup::RestoreService::new(data_dir, true);
        let _ = restore
            .restore_fill(
                engine,
                spacestorage_backup::RestoreRequest {
                    snapshot_id: man.snapshot_id,
                    confirm_drop: true,
                    pitr: None,
                    key_refs: man.key_refs.clone(),
                    wal_stamps: Default::default(),
                },
            )
            .await?;

        let target_id = Uuid::now_v7();
        {
            let content = engine.content.read().await;
            let exported = content.export_durable(source_id);
            drop(content);
            let mut content = engine.content.write().await;
            content.register_mode(target_id, StorageMode::Persistent);
            let mut objects = 0u64;
            let mut bytes = 0u64;
            for (k, v) in exported {
                bytes += (k.len() + v.len()) as u64;
                objects += 1;
                content.put(target_id, k, v);
            }
            // Clear restored source-id image so move/copy dest is the new id.
            content.clear_content(source_id);
            job.progress.objects_copied = objects;
            job.progress.bytes_copied = bytes;
        }

        job.snapshot_id = Some(man.snapshot_id);
        dest_names.insert(dest_key, target_id);
        if matches!(spec.policy, MigratePolicy::Move) {
            let src_key = (
                spec.source.namespace.clone(),
                spec.source.container.clone(),
            );
            dest_names.remove(&src_key);
        }
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

/// GDPR erase through the migrate orchestration seam — legal-hold gate first.
pub fn erase_with_legal(
    store: &Arc<Mutex<LegalHoldStore>>,
    req: EraseRequest,
) -> Result<EraseRecord, LegalError> {
    store.lock().erase(req)
}
