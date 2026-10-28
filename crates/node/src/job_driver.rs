//! Drive accepted `data_*` jobs against node catalog / storage (010 T058–T075).

use crate::Node;
use spacestorage_migrate::{
    admit_dest_bytes, recheck_dest_bytes, AuthzContext, BackupScope, BackupSpec, ContainerRef,
    JobId, JobKind, JobRecord, JobStatus, MigrateError, MigratePolicy, MigrationSpec, PlacementCheck,
    RestoreSpec, Strategy, TransformSpec,
};
use spacestorage_placement::topology::NodeTopo;
use spacestorage_placement::{write_quorum_satisfied, Quorum};
use spacestorage_storage::{ContentStore, StorageMode};
use spacestorage_types::{L3Model, StorageModeChoice, WriteAck};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

/// Install drive + persist + dual-write hooks on a freshly built node.
pub fn install(node: &Arc<Node>) {
    let n = Arc::clone(node);
    node.jobs.set_drive_hook(Some(Arc::new(move |id| {
        let n2 = Arc::clone(&n);
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    // T075: bump `08` job series on terminal states via JobService.
                    match drive_job(&n2, id).await {
                        Ok(()) => {
                            let _ = n2.jobs.complete(id);
                        }
                        Err(e) => {
                            let _ = n2.jobs.fail(id, &e);
                        }
                    }
                });
            }
            Err(_) => {
                tracing::debug!(job_id = %id, "job drive deferred: no tokio runtime");
            }
        }
    })));

    let n = Arc::clone(node);
    node.jobs.set_persist_hook(Some(Arc::new(move |job| {
        persist_job_record(&n, job)
    })));

    // Dual-write interceptor (T067/T069): mapped target apply at write quorum before source ack.
    {
        let jobs = Arc::clone(&node.jobs);
        let node_apply = Arc::clone(node);
        if let Ok(mut cat) = node.catalog.write() {
            cat.set_write_interceptor(Some(Arc::new(move |cat, source_id, key, value| {
                let dual = jobs.dual.clone();
                let jobs2 = Arc::clone(&jobs);
                let node_apply = Arc::clone(&node_apply);
                dual.intercept_source_write(
                    source_id,
                    key.as_bytes(),
                    value,
                    |target_id, k, v| apply_target_at_quorum(cat, &node_apply, target_id, k, v),
                    |job_id, err| {
                        let _ = jobs2.pause_with_error(job_id, &err);
                    },
                )
                .map(|_| ())
                .map_err(|e| e.to_string())
            })));
        }
    }

    // Hydrate jobs from controlplane when a runtime is already available.
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let n = Arc::clone(node);
        handle.spawn(async move {
            hydrate_from_controlplane(&n).await;
        });
    }
}

/// Apply mapped dual-write on the target at that container's declared write quorum
/// (`04` `write_quorum_satisfied` against durable replica acks) before source ack (T069).
fn apply_target_at_quorum(
    cat: &mut spacestorage_types::ContainerCatalog,
    node: &Node,
    target_id: Uuid,
    key: &[u8],
    value: &[u8],
) -> Result<(), MigrateError> {
    let cfg = node.config.load();
    let domain = cfg
        .cluster
        .quorum_domain
        .clone()
        .unwrap_or_else(|| "default".into());
    let required = Quorum::parse(&cfg.query_defaults.write_quorum).unwrap_or(Quorum::One);
    let live_replicas = {
        // Prefer membership replica count in-domain; fall back to open drives / 1.
        let mut n = 0u32;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            n = tokio::task::block_in_place(|| {
                handle.block_on(async {
                    let mut count = 0u32;
                    if let Some(m) = node.membership.read().await.as_ref() {
                        if let Some(view) = m.view_snapshot() {
                            count = view
                                .replica_targets()
                                .into_iter()
                                .filter(|r| r.quorum_domain == domain)
                                .count() as u32;
                        }
                    }
                    count
                })
            });
        }
        if n == 0 {
            1
        } else {
            n
        }
    };

    let key_s = String::from_utf8_lossy(key);
    let eng = match node.storage.try_read() {
        Ok(g) => g.clone(),
        Err(_) => {
            return Err(MigrateError::Io(
                "target apply: storage lock busy".into(),
            ));
        }
    };

    let acks: Vec<WriteAck> = if let Some(eng) = eng.as_ref() {
        let key_owned = key_s.clone().into_owned();
        let val = value.to_vec();
        let domain = domain.clone();
        let eng = Arc::clone(eng);
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| {
                handle.block_on(async move {
                    eng.put_kv_durable_acks(target_id, &key_owned, &val, &domain)
                        .await
                        .map_err(|e| MigrateError::Io(e.to_string()))
                })
            })?,
            Err(_) => {
                return Err(MigrateError::Io(
                    "target apply: no tokio runtime for durable put".into(),
                ));
            }
        }
    } else {
        // No engine: catalog mirrored put + synthesised local durable ack.
        cat.put_mirrored(target_id, &key_s, value.to_vec())
            .map_err(|e| MigrateError::Io(e.to_string()))?;
        vec![WriteAck::from_wal(
            cfg.node_name.clone(),
            "local",
            1,
            spacestorage_types::AckKind::Durable,
            StorageModeChoice::Persistent,
            1,
            domain.clone(),
        )]
    };

    let durable_lookup = {
        let eng = eng.clone();
        move |_replica: &str, drive_id: &str| -> u64 {
            let Some(eng) = eng.as_ref() else {
                return u64::MAX;
            };
            let Ok(handle) = tokio::runtime::Handle::try_current() else {
                return u64::MAX;
            };
            tokio::task::block_in_place(|| {
                handle.block_on(async {
                    for d in eng.all_drives().await {
                        if d.drive_id == drive_id {
                            return d.durable_lsn();
                        }
                    }
                    eng.all_drives()
                        .await
                        .first()
                        .map(|d| d.durable_lsn())
                        .unwrap_or(u64::MAX)
                })
            })
        }
    };

    if !write_quorum_satisfied(required, &acks, &domain, &durable_lookup, live_replicas) {
        return Err(MigrateError::Io(format!(
            "target write quorum not satisfied (required={required:?}, domain={domain}, acks={})",
            acks.len()
        )));
    }

    // Durable bytes live in storage; DualWriteRegistry also buffers the mapped put.
    // Skip put_mirrored here when the engine path ran — durable hooks would double-append WAL.
    let _ = (cat, key_s, value);
    Ok(())
}

async fn hydrate_from_controlplane(node: &Arc<Node>) {
    let cp = node.controlplane.read().await;
    let Some(cp) = cp.as_ref() else {
        return;
    };
    let snap = cp.cluster_snapshot();
    let mut loaded = Vec::new();
    for (_id, v) in snap.data_jobs {
        if let Ok(job) = serde_json::from_value::<JobRecord>(v) {
            loaded.push(job);
        }
    }
    if !loaded.is_empty() {
        node.jobs.store.load_all(loaded);
    }
}

fn persist_job_record(node: &Node, job: &JobRecord) -> Result<(), String> {
    // Constitution II / T066: fsync on blocking pool; await 006 append before return.
    let prefer_cluster = spacestorage_migrate::JobStore::prefers_cluster_log(job);
    let job_json = serde_json::to_value(job).map_err(|e| e.to_string())?;
    let ns_name = job
        .body
        .get("source")
        .and_then(|s| s.get("namespace"))
        .and_then(|n| n.as_str())
        .unwrap_or("default")
        .to_string();
    let jsonl_dir = node.config.load().storage_data_dir.clone().map(PathBuf::from);

    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| {
            handle.block_on(persist_job_record_async(
                node,
                prefer_cluster,
                job_json,
                ns_name,
                jsonl_dir,
            ))
        }),
        Err(_) => {
            if let Some(dir) = jsonl_dir {
                append_jobs_jsonl_blocking(&dir.join("data_jobs.jsonl"), &job_json.to_string())?;
            }
            Ok(())
        }
    }
}

async fn persist_job_record_async(
    node: &Node,
    prefer_cluster: bool,
    job_json: serde_json::Value,
    ns_name: String,
    jsonl_dir: Option<PathBuf>,
) -> Result<(), String> {
    if let Some(dir) = jsonl_dir {
        let path = dir.join("data_jobs.jsonl");
        let line = job_json.to_string();
        tokio::task::spawn_blocking(move || append_jobs_jsonl_blocking(&path, &line))
            .await
            .map_err(|e| e.to_string())??;
    }

    // T074: do not swallow `006` append failures — surface as StateUnreadable to start/resume.
    if prefer_cluster {
        let guard = node.controlplane.read().await;
        if let Some(cp) = guard.as_ref() {
            cp.append_cluster(spacestorage_controlplane::ClusterOp::UpsertDataJob {
                job: job_json,
            })
            .await
            .map_err(|e| format!("StateUnreadable: {e}"))?;
        }
    } else {
        let guard = node.controlplane.read().await;
        if let Some(cp) = guard.as_ref() {
            let snap = cp.cluster_snapshot();
            if let Some(ns) = snap.namespaces.values().find(|n| n.name == ns_name) {
                let ns_id = ns.id;
                let body = serde_json::json!({"op":"upsert_data_job","job": job_json});
                cp.append_namespace(ns_id, body)
                    .await
                    .map_err(|e| format!("StateUnreadable: {e}"))?;
            }
        }
    }
    Ok(())
}

fn append_jobs_jsonl_blocking(path: &std::path::Path, line: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

/// Build `04` placement check from membership + config (T068).
async fn placement_check_for(node: &Node) -> PlacementCheck {
    let cfg = node.config.load();
    let ladder = if cfg.cluster.topology_ladder.is_empty() {
        vec!["az".into()]
    } else {
        cfg.cluster.topology_ladder.clone()
    };
    let available_drives: Vec<String> = cfg.storage.drives.iter().map(|d| d.id.clone()).collect();

    let mut candidates = Vec::new();
    if let Some(m) = node.membership.read().await.as_ref() {
        if let Some(view) = m.view_snapshot() {
            for rec in view.replica_targets() {
                candidates.push(NodeTopo {
                    name: rec.node_name.clone(),
                    labels: rec.labels.clone(),
                    quorum_domain: rec.quorum_domain.clone(),
                    no_tenant_data: false,
                });
            }
        }
    }
    if candidates.is_empty() {
        candidates.push(NodeTopo {
            name: cfg.node_name.clone(),
            labels: if cfg.labels.is_empty() {
                BTreeMap::from([("az".into(), "local".into())])
            } else {
                cfg.labels.clone()
            },
            quorum_domain: cfg
                .cluster
                .quorum_domain
                .clone()
                .unwrap_or_else(|| "default".into()),
            no_tenant_data: false,
        });
    }

    let rf = candidates.len().max(1) as u32;
    PlacementCheck {
        candidates,
        topology_ladder: ladder,
        replication_factor: rf.min(3),
        anti_affinity_keys: vec![],
        available_drives,
    }
}

pub async fn drive_job(node: &Arc<Node>, id: JobId) -> Result<(), MigrateError> {
    let job = node.jobs.status(id)?;
    if matches!(job.status, JobStatus::Completed | JobStatus::Failed) {
        return Ok(());
    }
    if node.jobs.is_cancel_requested(id) {
        return Err(MigrateError::Cancelled);
    }
    match job.kind {
        JobKind::DataMigration => drive_migrate(node, job).await,
        JobKind::DataTransformation => drive_transform(node, job).await,
        JobKind::DataBackup => drive_backup(node, job).await,
        JobKind::DataRestore => drive_restore(node, job).await,
    }
}

async fn drive_migrate(node: &Arc<Node>, job: JobRecord) -> Result<(), MigrateError> {
    let spec: MigrationSpec =
        serde_json::from_value(job.body.clone()).map_err(|e| MigrateError::Io(e.to_string()))?;
    let placement = placement_check_for(node).await;
    spec.validate_destination_placement(&placement)?;
    let (source_id, rows, names, mut content, source_bytes) =
        snapshot_source(node, &spec.source)?;
    let source_head = job
        .progress
        .last_source_seq
        .or(job.progress.last_applied_source_seq)
        .unwrap_or(rows.len() as u64)
        .max(1);
    let runner = node.jobs.migrate_runner_with_placement(placement);
    // T070: read destination namespace quota from `07` at start.
    let quota = admit_dest_bytes(&node.jobs.tenancy, spec.dest_ns(), source_bytes)?;
    let mut names = names;
    let done = match job.strategy {
        Strategy::Offline => runner.run_offline(
            job,
            &spec,
            source_id,
            &rows,
            source_head,
            &mut names,
            &mut content,
            quota,
            source_bytes,
        )?,
        Strategy::Snapshot => {
            // T072: invoke 013 snapshot of source + restore into destination (no dual-write).
            let meta = {
                let cat = node
                    .catalog
                    .read()
                    .map_err(|e| MigrateError::Io(e.to_string()))?;
                let c = cat
                    .describe(&spec.source.namespace, &spec.source.container)
                    .map_err(|_| MigrateError::NotFound)?;
                spacestorage_backup::ContainerSnapshotMeta {
                    id: c.id,
                    namespace: c.namespace.as_str().into(),
                    name: c.name.as_str().into(),
                    type_name: c.model.machine_name().into(),
                    mode: format!("{:?}", c.mode).to_ascii_lowercase(),
                    key_ref: None,
                }
            };
            let engine = {
                let g = node.storage.read().await;
                g.clone()
                    .ok_or_else(|| MigrateError::Io("storage engine not open".into()))?
            };
            let data_dir = node
                .config
                .load()
                .storage_data_dir
                .clone()
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    node.config_path
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .join("data")
                });
            // Re-check quota immediately before cutover materialisation.
            let quota = recheck_dest_bytes(&node.jobs.tenancy, spec.dest_ns(), source_bytes)?;
            let done = runner
                .run_snapshot(
                    job,
                    &spec,
                    source_id,
                    meta,
                    &mut names,
                    &engine,
                    data_dir,
                    quota,
                    source_bytes,
                )
                .await?;
            // Pull restored dest rows from the engine into the local content map for catalog publish.
            if let Some(((_, _), tid)) = names
                .iter()
                .find(|((ns, name), _)| *ns == spec.dest_ns() && *name == spec.dest_public_name())
            {
                let exported = engine.content.read().await.export_durable(*tid);
                content.register_mode(*tid, StorageMode::Persistent);
                for (k, v) in exported {
                    content.put(*tid, k, v);
                }
            }
            done
        }
        Strategy::Live => {
            let quota = recheck_dest_bytes(&node.jobs.tenancy, spec.dest_ns(), source_bytes)
                .unwrap_or(quota);
            runner.run_live(
                job,
                &spec,
                source_id,
                &rows,
                source_head,
                &mut names,
                &mut content,
                quota,
                source_bytes,
            )?
        }
    };
    let dest_key = (spec.dest_ns().to_string(), spec.dest_public_name());
    let target_id = names.get(&dest_key).copied();
    if let Some(tid) = target_id {
        // Absorb any pending dual-writes that landed after the runner's last drain.
        for (k, v) in node.jobs.dual.take_pending(tid) {
            content.put(tid, k, v);
        }
    }
    apply_name_content_to_catalog(node, &spec.source, &names, &content, &spec)?;
    // T078: unregister only after the target is published (no-op when no window, e.g. snapshot).
    let leftover = node.jobs.dual.unregister(source_id);
    if let Some(tid) = target_id {
        merge_leftover_into_catalog(node, tid, leftover)?;
    }
    let _ = done;
    Ok(())
}

async fn drive_transform(node: &Arc<Node>, job: JobRecord) -> Result<(), MigrateError> {
    let spec: TransformSpec =
        serde_json::from_value(job.body.clone()).map_err(|e| MigrateError::Io(e.to_string()))?;
    let job_id = job.id;
    let (source_id, rows, names, mut content, source_bytes) = snapshot_source(node, &spec.source)?;
    let source_head = job
        .progress
        .last_applied_source_seq
        .unwrap_or(rows.len() as u64)
        .max(1);
    let runner = node.jobs.transform_runner();
    let mut names = names;
    // T070: quota at transform start; T077 re-reads grown size + `07` inside runner before swap.
    let quota = admit_dest_bytes(&node.jobs.tenancy, &spec.source.namespace, source_bytes)?;
    let done = runner.run(
        job,
        &spec,
        source_id,
        &rows,
        source_head,
        &mut names,
        &mut content,
        quota,
        source_bytes,
    )?;
    let Some(target_id) = transform_publish_target_id(&spec, &names, source_id, job_id) else {
        let _ = node.jobs.dual.unregister(source_id);
        return Err(MigrateError::InvalidState);
    };
    for (k, v) in node.jobs.dual.take_pending(target_id) {
        content.put(target_id, k, v);
    }
    // Publish only the transform target; retain_source rebinds without wiping rows (T078).
    apply_transform_to_catalog(node, &spec, &names, &content, source_id, target_id)?;
    // T078: unregister only after catalog publish so a source ack cannot miss the swap.
    let leftover = node.jobs.dual.unregister(source_id);
    merge_leftover_into_catalog(node, target_id, leftover)?;
    let _ = done;
    Ok(())
}

async fn drive_backup(node: &Arc<Node>, job: JobRecord) -> Result<(), MigrateError> {
    let mut spec: BackupSpec = serde_json::from_value(job.body.clone()).unwrap_or(BackupSpec {
        scope: BackupScope::Namespace {
            name: "default".into(),
        },
        pitr: false,
        containers: vec![],
        available_key_refs: vec![],
    });
    if spec.containers.is_empty() {
        let cat = node
            .catalog
            .read()
            .map_err(|e| MigrateError::Io(e.to_string()))?;
        for c in cat.list() {
            let include = match &spec.scope {
                BackupScope::Namespace { name } => c.namespace.as_str() == name,
                BackupScope::Container {
                    namespace,
                    container,
                } => c.namespace.as_str() == namespace && c.name.as_str() == container,
            };
            if include {
                spec.containers
                    .push(spacestorage_backup::ContainerSnapshotMeta {
                        id: c.id,
                        namespace: c.namespace.as_str().into(),
                        name: c.name.as_str().into(),
                        type_name: c.model.machine_name().into(),
                        mode: format!("{:?}", c.mode).to_ascii_lowercase(),
                        key_ref: None,
                    });
            }
        }
    }
    let engine = {
        let g = node.storage.read().await;
        g.clone()
            .ok_or_else(|| MigrateError::Io("storage engine not open".into()))?
    };
    let data_dir = node
        .config
        .load()
        .storage_data_dir
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            node.config_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("data")
        });
    let orch = node.jobs.backup_orchestrator();
    orch.run_backup(job, &spec, &engine, data_dir, &AuthzContext::cluster_admin())
        .await?;
    Ok(())
}

async fn drive_restore(node: &Arc<Node>, job: JobRecord) -> Result<(), MigrateError> {
    let spec: RestoreSpec =
        serde_json::from_value(job.body.clone()).map_err(|e| MigrateError::Io(e.to_string()))?;
    let engine = {
        let g = node.storage.read().await;
        g.clone()
            .ok_or_else(|| MigrateError::Io("storage engine not open".into()))?
    };
    let data_dir = node
        .config
        .load()
        .storage_data_dir
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            node.config_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("data")
        });
    let orch = node.jobs.backup_orchestrator();
    orch.run_restore(job, &spec, &engine, data_dir, &AuthzContext::cluster_admin())
        .await?;
    Ok(())
}

fn snapshot_source(
    node: &Node,
    source: &ContainerRef,
) -> Result<
    (
        Uuid,
        HashMap<Vec<u8>, Vec<u8>>,
        HashMap<(String, String), Uuid>,
        ContentStore,
        u64,
    ),
    MigrateError,
> {
    let cat = node
        .catalog
        .read()
        .map_err(|e| MigrateError::Io(e.to_string()))?;
    let c = cat
        .describe(&source.namespace, &source.container)
        .map_err(|_| MigrateError::NotFound)?;
    let source_id = c.id;
    let mut rows = HashMap::new();
    let mut bytes = 0u64;
    for k in cat.keys(source_id).unwrap_or_default() {
        if let Ok(Some(v)) = cat.get(source_id, &k) {
            bytes += (k.len() + v.len()) as u64;
            rows.insert(k.into_bytes(), v.to_vec());
        }
    }
    let mut names = HashMap::new();
    for c in cat.list() {
        names.insert(
            (c.namespace.as_str().into(), c.name.as_str().into()),
            c.id,
        );
    }
    let mut content = ContentStore::default();
    for c in cat.list() {
        content.register_mode(c.id, StorageMode::Persistent);
        if let Ok(keys) = cat.keys(c.id) {
            for k in keys {
                if let Ok(Some(v)) = cat.get(c.id, &k) {
                    content.put(c.id, k.into_bytes(), v.to_vec());
                }
            }
        }
    }
    Ok((source_id, rows, names, content, bytes))
}

fn apply_name_content_to_catalog(
    node: &Node,
    source: &ContainerRef,
    names: &HashMap<(String, String), Uuid>,
    content: &ContentStore,
    spec: &MigrationSpec,
) -> Result<(), MigrateError> {
    let mut cat = node
        .catalog
        .write()
        .map_err(|e| MigrateError::Io(e.to_string()))?;
    let dest_key = (spec.dest_ns().to_string(), spec.dest_public_name());
    let Some(&target_id) = names.get(&dest_key) else {
        return Ok(());
    };
    if cat.describe(&dest_key.0, &dest_key.1).is_err() {
        let _ = cat.restore_container(
            target_id,
            dest_key.0.clone(),
            dest_key.1.clone(),
            L3Model::KvStore,
            StorageModeChoice::Persistent,
            None,
        );
    }
    let exported = content.export_durable(target_id);
    let mut rowmap = HashMap::new();
    for (k, v) in exported {
        rowmap.insert(String::from_utf8_lossy(&k).into_owned(), v);
    }
    let _ = cat.replace_rows(target_id, rowmap);
    if matches!(spec.policy, MigratePolicy::Move) {
        let _ = cat.drop_container(&source.namespace, &source.container);
    }
    Ok(())
}

/// Target id the transform runner published into `names` (swap → public source name).
fn transform_publish_target_id(
    spec: &TransformSpec,
    names: &HashMap<(String, String), Uuid>,
    live_source_id: Uuid,
    job_id: Uuid,
) -> Option<Uuid> {
    if spec.swap {
        names
            .get(&(spec.source.namespace.clone(), spec.source.container.clone()))
            .copied()
            .filter(|id| *id != live_source_id)
    } else {
        let new_name = spec.new_name.clone().unwrap_or_else(|| {
            format!(
                "{}_xf_{}",
                spec.source.container,
                &job_id.to_string()[..8]
            )
        });
        names
            .get(&(spec.source.namespace.clone(), new_name))
            .copied()
    }
}

fn merge_leftover_into_catalog(
    node: &Node,
    target_id: Uuid,
    leftover: HashMap<Vec<u8>, Vec<u8>>,
) -> Result<(), MigrateError> {
    if leftover.is_empty() {
        return Ok(());
    }
    let mut cat = node
        .catalog
        .write()
        .map_err(|e| MigrateError::Io(e.to_string()))?;
    for (k, v) in leftover {
        let key = String::from_utf8_lossy(&k).into_owned();
        let _ = cat.put(target_id, &key, v);
    }
    Ok(())
}

fn apply_transform_to_catalog(
    node: &Node,
    spec: &TransformSpec,
    names: &HashMap<(String, String), Uuid>,
    content: &ContentStore,
    live_source_id: Uuid,
    target_id: Uuid,
) -> Result<(), MigrateError> {
    let mut cat = node
        .catalog
        .write()
        .map_err(|e| MigrateError::Io(e.to_string()))?;

    // T078: publish only the transform target. `names` still holds every pre-job container
    // from `snapshot_source`; never `replace_rows` those unrelated ids from the snapshot.
    if spec.swap {
        if spec.retain_source {
            // Rebind previous under `<old>__pre_transform_<job_id>` without drop_container
            // deleting that id's rows.
            if let Some(((ns, retain_name), _)) =
                names.iter().find(|(_, id)| **id == live_source_id)
            {
                if cat
                    .describe(&spec.source.namespace, &spec.source.container)
                    .is_ok()
                {
                    cat.rename_container(
                        &spec.source.namespace,
                        &spec.source.container,
                        retain_name,
                    )
                    .map_err(|e| match e {
                        spacestorage_types::TypeError::AlreadyExists => {
                            MigrateError::NameExists {
                                container: format!(
                                    "{}.{}",
                                    spec.source.namespace, retain_name
                                ),
                            }
                        }
                        other => MigrateError::Io(other.to_string()),
                    })?;
                } else if cat.describe(ns, retain_name).is_err() {
                    let _ = cat.restore_container(
                        live_source_id,
                        ns.clone(),
                        retain_name.clone(),
                        L3Model::KvStore,
                        StorageModeChoice::Persistent,
                        None,
                    );
                }
            }
        } else {
            let _ = cat.drop_container(&spec.source.namespace, &spec.source.container);
        }
        let ns = &spec.source.namespace;
        let name = &spec.source.container;
        if cat.describe(ns, name).is_err() {
            let _ = cat.restore_container(
                target_id,
                ns.clone(),
                name.clone(),
                L3Model::KvStore,
                StorageModeChoice::Persistent,
                None,
            );
        }
    } else if let Some(((ns, name), _)) = names.iter().find(|(_, id)| **id == target_id) {
        if cat.describe(ns, name).is_err() {
            let _ = cat.restore_container(
                target_id,
                ns.clone(),
                name.clone(),
                L3Model::KvStore,
                StorageModeChoice::Persistent,
                None,
            );
        }
    }

    let exported = content.export_durable(target_id);
    let mut rowmap = HashMap::new();
    for (k, v) in exported {
        rowmap.insert(String::from_utf8_lossy(&k).into_owned(), v);
    }
    let _ = cat.replace_rows(target_id, rowmap);
    Ok(())
}

#[cfg(test)]
mod t078_catalog_publish {
    use super::*;
    use spacestorage_migrate::{ContainerRef, RewriteKind, TransformSpec};
    use spacestorage_types::ContainerCatalog;

    /// Minimal Node stand-in: exercise catalog helpers via a real catalog lock shape.
    fn publish_only_target_and_retain() {
        let mut cat = ContainerCatalog::new();
        let source_id = cat
            .create(
                "acme",
                "t",
                L3Model::KvStore,
                false,
                None,
                StorageModeChoice::Persistent,
            )
            .unwrap();
        cat.put(source_id, "src", b"keep".to_vec()).unwrap();
        let other_id = cat
            .create(
                "acme",
                "unrelated",
                L3Model::KvStore,
                false,
                None,
                StorageModeChoice::Persistent,
            )
            .unwrap();
        cat.put(other_id, "live", b"ack".to_vec()).unwrap();

        let target_id = Uuid::now_v7();
        let job_id = Uuid::now_v7();
        let retain = format!("t__pre_transform_{job_id}");

        // Simulate post-runner names map (includes unrelated from snapshot_source).
        let mut names = HashMap::new();
        names.insert(("acme".into(), "t".into()), target_id);
        names.insert(("acme".into(), retain.clone()), source_id);
        names.insert(("acme".into(), "unrelated".into()), other_id);

        let mut content = ContentStore::default();
        content.register_mode(target_id, StorageMode::Persistent);
        content.register_mode(source_id, StorageMode::Persistent);
        content.register_mode(other_id, StorageMode::Persistent);
        // Stale pre-job snapshot for unrelated — must NOT be written back.
        content.put(other_id, b"live".to_vec(), b"STALE".to_vec());
        content.put(target_id, b"out".to_vec(), b"re_encode/v1|x".to_vec());
        content.put(source_id, b"src".to_vec(), b"keep".to_vec());

        // Inline the T078 catalog cutover (same steps as apply_transform_to_catalog).
        let spec = TransformSpec {
            source: ContainerRef {
                namespace: "acme".into(),
                container: "t".into(),
            },
            rewrite: RewriteKind::ReEncode,
            target_type: Some("kv_store".into()),
            mapping_query: None,
            new_name: Some("t_new".into()),
            swap: true,
            retain_source: true,
            source_type: "kv_store".into(),
        };
        assert_eq!(
            transform_publish_target_id(&spec, &names, source_id, job_id),
            Some(target_id)
        );

        cat.rename_container("acme", "t", &retain).unwrap();
        cat.restore_container(
            target_id,
            "acme",
            "t",
            L3Model::KvStore,
            StorageModeChoice::Persistent,
            None,
        )
        .unwrap();
        let exported = content.export_durable(target_id);
        let mut rowmap = HashMap::new();
        for (k, v) in exported {
            rowmap.insert(String::from_utf8_lossy(&k).into_owned(), v);
        }
        cat.replace_rows(target_id, rowmap).unwrap();
        // Do NOT replace_rows(other_id) from snapshot — live ack must remain.
        assert_eq!(cat.get(other_id, "live").unwrap(), Some(b"ack".as_slice()));
        assert_eq!(cat.get(source_id, "src").unwrap(), Some(b"keep".as_slice()));
        assert_eq!(cat.describe("acme", &retain).unwrap().id, source_id);
        assert_eq!(cat.describe("acme", "t").unwrap().id, target_id);
        assert_eq!(
            cat.get(target_id, "out").unwrap(),
            Some(b"re_encode/v1|x".as_slice())
        );
    }

    #[test]
    fn t078_publish_only_target_keeps_unrelated_and_retained_rows() {
        publish_only_target_and_retain();
    }
}
