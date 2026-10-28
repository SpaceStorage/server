//! TransformSpec + five rewrite kinds + swap (010 US3).

use crate::authz::AuthzContext;
use crate::dual_write::{incomplete_name, DualWriteRegistry, DualWriteRewrite, DualWriteWindow};
use crate::error::MigrateError;
use crate::job::{JobKind, JobRecord, Strategy};
use crate::mapping::{
    apply_row_transform, execute_mapping_query, resolve_mapping, MappingQuery, MappingSpec,
};
use crate::quota::{measure_row_bytes, refresh_quota_view, QuotaView};
use crate::store::JobStore;
use serde::{Deserialize, Serialize};
use spacestorage_storage::{ContentStore, StorageMode};
use spacestorage_tenancy::Tenancy;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::migrate::ContainerRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteKind {
    TypeModel,
    IncompatibleSchema,
    ReEncode,
    ReCompress,
    ReEncrypt,
    ShardingKey,
}

impl RewriteKind {
    pub fn parse(s: &str) -> Result<Self, MigrateError> {
        match s {
            "type_model" | "type-model" => Ok(Self::TypeModel),
            "incompatible_schema" | "schema" => Ok(Self::IncompatibleSchema),
            "re_encode" | "re-encode" => Ok(Self::ReEncode),
            "re_compress" | "re-compress" => Ok(Self::ReCompress),
            "re_encrypt" | "re-encrypt" => Ok(Self::ReEncrypt),
            "sharding_key" | "sharding-key" => Ok(Self::ShardingKey),
            other => Err(MigrateError::NotSupported {
                what: format!("rewrite {other}"),
            }),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TypeModel => "type_model",
            Self::IncompatibleSchema => "incompatible_schema",
            Self::ReEncode => "re_encode",
            Self::ReCompress => "re_compress",
            Self::ReEncrypt => "re_encrypt",
            Self::ShardingKey => "sharding_key",
        }
    }

    pub fn identity_ok(self) -> bool {
        matches!(
            self,
            Self::ReEncode | Self::ReCompress | Self::ReEncrypt | Self::ShardingKey
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformSpec {
    pub source: ContainerRef,
    pub rewrite: RewriteKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_query: Option<MappingQuery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(default = "default_true")]
    pub swap: bool,
    #[serde(default)]
    pub retain_source: bool,
    #[serde(default)]
    pub source_type: String,
}

fn default_true() -> bool {
    true
}

impl TransformSpec {
    pub fn validate(&self, authz: &AuthzContext) -> Result<(), MigrateError> {
        authz.allow_same_ns_transform()?;
        let to = self
            .target_type
            .clone()
            .unwrap_or_else(|| self.source_type.clone());
        let complex = matches!(self.rewrite, RewriteKind::TypeModel | RewriteKind::IncompatibleSchema)
            && !self.rewrite.identity_ok();
        let _ = resolve_mapping(
            &self.source_type,
            &to,
            self.mapping_query.as_ref(),
            complex || matches!(self.rewrite, RewriteKind::TypeModel),
        )?;
        if matches!(self.rewrite, RewriteKind::TypeModel) && self.target_type.is_none() {
            return Err(MigrateError::NotSupported {
                what: "type_model without --to-type".into(),
            });
        }
        Ok(())
    }
}

pub struct TransformRunner {
    pub store: JobStore,
    pub dual: DualWriteRegistry,
    pub slice10: bool,
    pub tenancy: Arc<Tenancy>,
}

impl TransformRunner {
    pub fn run(
        &self,
        mut job: JobRecord,
        spec: &TransformSpec,
        source_id: Uuid,
        source_rows: &HashMap<Vec<u8>, Vec<u8>>,
        source_head: u64,
        names: &mut HashMap<(String, String), Uuid>,
        content: &mut ContentStore,
        quota: QuotaView,
        source_bytes: u64,
    ) -> Result<JobRecord, MigrateError> {
        if !self.slice10 {
            return Err(MigrateError::MigrateSlice10Required);
        }
        spec.validate(&AuthzContext::cluster_admin())?;
        // FR-011 / R11: refuse before any copy when a second copy does not fit.
        quota.check_fits(source_bytes)?;

        let new_name = spec
            .new_name
            .clone()
            .unwrap_or_else(|| format!("{}_xf_{}", spec.source.container, &job.id.to_string()[..8]));
        let dest_key = (spec.source.namespace.clone(), new_name.clone());
        if names.contains_key(&dest_key) {
            return Err(MigrateError::NameExists {
                container: format!("{}.{}", dest_key.0, dest_key.1),
            });
        }

        let to = spec
            .target_type
            .clone()
            .unwrap_or_else(|| spec.source_type.clone());
        let complex = matches!(
            spec.rewrite,
            RewriteKind::TypeModel | RewriteKind::IncompatibleSchema
        );
        let mapping: MappingSpec = resolve_mapping(
            &spec.source_type,
            &to,
            spec.mapping_query.as_ref(),
            complex,
        )?;

        let target_id = Uuid::now_v7();
        job.target_internal_name = Some(incomplete_name(job.id));
        content.register_mode(target_id, StorageMode::Persistent);
        self.dual.register(DualWriteWindow {
            container_id: source_id,
            job_id: job.id,
            install_seq: source_head,
            target_id,
            rewrite: Some(DualWriteRewrite {
                kind: spec.rewrite.as_str().into(),
                mapping: mapping.clone(),
            }),
        })?;
        job.mark_running();
        job.progress.install_seq = Some(source_head);
        job.progress.last_source_seq = Some(source_head);

        let mut objects = 0u64;
        let mut bytes = 0u64;
        match &mapping {
            MappingSpec::Query(q) => {
                // FR-014 / R13: evaluate via 005 LogicalRequest through crates/query.
                let projected = execute_mapping_query(q, source_rows)?;
                for (nk, nv) in projected {
                    content.put(target_id, nk.clone(), nv.clone());
                    objects += 1;
                    bytes += (nk.len() + nv.len()) as u64;
                }
            }
            MappingSpec::Catalog(_) => {
                for (k, v) in source_rows {
                    let (nk, nv) = apply_row_transform(spec.rewrite.as_str(), &mapping, k, v);
                    content.put(target_id, nk.clone(), nv.clone());
                    objects += 1;
                    bytes += (nk.len() + nv.len()) as u64;
                }
            }
        }
        job.progress.objects_copied = objects;
        job.progress.bytes_copied = bytes;
        job.progress.last_applied_source_seq = Some(source_head);

        // Drain mapped dual-writes into the target (including puts after the first drain).
        let mut last_applied = source_head;
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
        job.progress.last_applied_source_seq = Some(last_applied.max(source_head));

        let effective_head = source_head.max(last_applied);
        if last_applied.max(source_head) < effective_head {
            let _ = self.dual.unregister(source_id);
            let e = MigrateError::InvalidState;
            job.mark_failed(&e);
            self.store.insert(job.clone());
            return Err(e);
        }

        // T077: re-read `07` usage and grown source size immediately before swap.
        let grown = self
            .dual
            .grown_source_bytes(source_id, source_bytes)
            .max(measure_row_bytes(source_rows));
        let quota = refresh_quota_view(&self.tenancy, &spec.source.namespace, quota);
        if let Err(e) = quota.check_fits(grown) {
            let _ = self.dual.unregister(source_id);
            job.mark_failed(&e);
            self.store.insert(job.clone());
            return Err(e);
        }

        names.insert(dest_key.clone(), target_id);

        if spec.swap {
            let src_key = (
                spec.source.namespace.clone(),
                spec.source.container.clone(),
            );
            if let Some(old_id) = names.remove(&src_key) {
                names.insert(src_key, target_id);
                if !spec.retain_source {
                    content.clear_content(old_id);
                } else {
                    let retain = format!(
                        "{}__pre_transform_{}",
                        spec.source.container, job.id
                    );
                    let rk = (spec.source.namespace.clone(), retain);
                    if names.contains_key(&rk) {
                        let _ = self.dual.unregister(source_id);
                        let e = MigrateError::NameExists {
                            container: format!("{}.{}", rk.0, rk.1),
                        };
                        job.mark_failed(&e);
                        self.store.insert(job.clone());
                        return Err(e);
                    }
                    names.insert(rk, old_id);
                }
            }
        }

        // Final drain into content while the dual-write window stays registered.
        // T078: unregister only after catalog publish so a source ack cannot miss the swap.
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
        job.strategy = Strategy::Live;
        self.store.insert(job.clone());
        let _ = JobKind::DataTransformation;
        Ok(job)
    }
}
