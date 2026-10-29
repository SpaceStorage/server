//! PlannerEngine — default production QueryEngine (005).

use crate::admission::{AdmissionController, AdmissionLimits};
use crate::cancel::{CancelToken, check_deadline};
use crate::catalog_exec::{
    QueryResult, SharedCatalog, copy_in, copy_out, ensure_container, execute_adhoc_sql,
    object_delete, object_get, object_put, object_scan, parse_set_transaction_isolation,
};
use crate::error::ExecError;
use crate::options::{IsolationLevel, QueryOptions};
use crate::planner::{self, explain_text};
use crate::record::{ExecutionRecord, ExecutionStage};
use crate::request::LogicalRequest;
use crate::schedule;
use crate::subscribe::{JobRegistry, SharedJobRegistry};
use crate::txn::{SharedTxnRegistry, TxnRegistry, commit_distributed};
use parking_lot::Mutex;
use spacestorage_compat::{ClassifyOutcome, DialectProfile};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use uuid::Uuid;

/// Process-wide txn/job registries so handlers constructing a fresh
/// `PlannerEngine` per statement still share session txn state.
fn shared_txns() -> SharedTxnRegistry {
    static TXNS: OnceLock<SharedTxnRegistry> = OnceLock::new();
    Arc::clone(TXNS.get_or_init(|| Arc::new(TxnRegistry::new())))
}

fn shared_jobs() -> SharedJobRegistry {
    static JOBS: OnceLock<SharedJobRegistry> = OnceLock::new();
    Arc::clone(JOBS.get_or_init(|| Arc::new(JobRegistry::new())))
}

/// Shared query engine trait — handlers call only this + LogicalRequest.
#[async_trait::async_trait]
pub trait QueryEngine: Send + Sync {
    async fn execute(
        &self,
        namespace: &str,
        session: &str,
        protocol: &str,
        request: LogicalRequest,
        options: QueryOptions,
        cancel: CancelToken,
    ) -> Result<QueryResult, ExecError>;

    fn explain(&self, request: LogicalRequest, options: QueryOptions) -> Result<String, ExecError> {
        let (lp, pp) = planner::plan(request, options)?;
        Ok(explain_text(&lp, &pp))
    }

    fn cancel(&self, exec_id: Uuid) -> bool {
        let _ = exec_id;
        false
    }

    fn subscribe_wait(&self, exec_id: Uuid) -> Result<crate::subscribe::Subscription, ExecError> {
        Err(ExecError::NotSupported(format!("subscribe_wait:{exec_id}")))
    }
}

pub struct PlannerEngine {
    catalog: SharedCatalog,
    admission: AdmissionController,
    txns: SharedTxnRegistry,
    jobs: SharedJobRegistry,
    records: Arc<Mutex<VecDeque<ExecutionRecord>>>,
    coordinator: String,
    data_dir: Option<PathBuf>,
    #[cfg(feature = "query-distributed")]
    shuffle: crate::engines::shuffle::ShuffleTransport,
    /// When false, BEGIN/COPY/distributed engines refuse (first-binary / HC dialect).
    distributed: bool,
    tenancy: Option<Arc<spacestorage_tenancy::Tenancy>>,
}

impl PlannerEngine {
    pub fn new(catalog: SharedCatalog) -> Self {
        Self::with_limits(catalog, AdmissionLimits::default(), false)
    }

    pub fn with_distributed(catalog: SharedCatalog, distributed: bool) -> Self {
        let mut limits = AdmissionLimits::default();
        // Complete-product (distributed) spills sort/hash/agg past query memory (FR-010).
        if distributed {
            limits.spill = true;
        }
        Self::with_limits(catalog, limits, distributed)
    }

    pub fn with_limits(catalog: SharedCatalog, limits: AdmissionLimits, distributed: bool) -> Self {
        Self {
            catalog,
            admission: AdmissionController::new(limits),
            txns: shared_txns(),
            jobs: shared_jobs(),
            records: Arc::new(Mutex::new(VecDeque::with_capacity(1024))),
            coordinator: "local".into(),
            data_dir: None,
            #[cfg(feature = "query-distributed")]
            shuffle: crate::engines::shuffle::ShuffleTransport::new(),
            distributed,
            tenancy: None,
        }
    }

    pub fn with_tenancy(mut self, tenancy: Arc<spacestorage_tenancy::Tenancy>) -> Self {
        self.tenancy = Some(tenancy);
        self
    }

    pub fn set_data_dir(&mut self, dir: PathBuf) {
        self.data_dir = Some(dir);
    }

    pub fn jobs(&self) -> SharedJobRegistry {
        Arc::clone(&self.jobs)
    }

    pub fn txns(&self) -> SharedTxnRegistry {
        Arc::clone(&self.txns)
    }

    pub fn records_snapshot(&self) -> Vec<ExecutionRecord> {
        self.records.lock().iter().cloned().collect()
    }

    fn push_record(&self, rec: ExecutionRecord) {
        let mut q = self.records.lock();
        if q.len() >= 10_000 {
            q.pop_front();
        }
        q.push_back(rec);
    }

    fn require_distributed(&self, feature: &str) -> Result<(), ExecError> {
        if self.distributed {
            Ok(())
        } else {
            Err(ExecError::NotSupported(format!(
                "{feature} requires query-distributed / complete-product"
            )))
        }
    }

    /// Refuse SNAPSHOT when a touched type is not snapshot-capable; freeze rows for capable types.
    fn ensure_snapshot_for_sql(
        &self,
        namespace: &str,
        session: &str,
        sql: &str,
    ) -> Result<(), ExecError> {
        let upper = sql.to_ascii_uppercase();
        let table = extract_table_from_dml(&upper, sql);
        let Some(table) = table else {
            return Ok(());
        };
        self.txns.with_mut(session, |txn| {
            if txn.snapshot_checked.iter().any(|t| t == &table) {
                return Ok(());
            }
            let cat = self.catalog.read().expect("catalog");
            let container = match cat.describe(namespace, &table) {
                Ok(c) => c,
                Err(_) => {
                    // Table may be created later in the txn; defer capability check.
                    return Ok(());
                }
            };
            let model = container.model;
            let type_name = model.display_name().to_string();
            let capable = model.snapshot_capable();
            spacestorage_compat::check_snapshot_capable(&type_name, capable).map_err(
                |e| match e {
                    spacestorage_compat::CompatError::SnapshotUnsupported { type_name } => {
                        ExecError::SnapshotUnsupported(type_name)
                    }
                    other => ExecError::Isolation(other.to_string()),
                },
            )?;
            // Freeze current rows for subsequent SELECTs in this SNAPSHOT txn.
            if let Ok(id) = cat.describe(namespace, &table).map(|c| c.id) {
                if let Ok(keys) = cat.keys(id) {
                    let mut snap = std::collections::HashMap::new();
                    for k in keys {
                        if let Ok(Some(v)) = cat.get(id, &k) {
                            snap.insert(k, v.to_vec());
                        }
                    }
                    txn.read_snapshots.insert(table.clone(), snap);
                }
            }
            txn.snapshot_checked.push(table);
            Ok(())
        })
    }

    fn snapshot_select(
        &self,
        namespace: &str,
        session: &str,
        sql: &str,
    ) -> Option<Result<QueryResult, ExecError>> {
        let upper = sql.trim_start().to_ascii_uppercase();
        if !upper.starts_with("SELECT") {
            return None;
        }
        let txn = self.txns.get(session)?;
        if txn.isolation != IsolationLevel::Snapshot || txn.state != crate::txn::TxnState::Open {
            return None;
        }
        let table = extract_table_from_dml(&upper, sql)?;
        let snap = txn.read_snapshots.get(&table)?;
        // Materialize frozen rows as SELECT result (key, value columns).
        let mut rows = Vec::new();
        for (k, v) in snap {
            let text = String::from_utf8_lossy(v).into_owned();
            rows.push(vec![Some(k.clone()), Some(text)]);
        }
        let _ = namespace;
        Some(Ok(QueryResult {
            tag: format!("SELECT {}", rows.len()),
            columns: vec!["id".into(), "v".into()],
            rows,
        }))
    }

    /// Synchronous execute path (handlers + unit tests).
    pub fn execute_blocking(
        &self,
        namespace: &str,
        session: &str,
        protocol: &str,
        request: LogicalRequest,
        options: QueryOptions,
        cancel: CancelToken,
    ) -> Result<QueryResult, ExecError> {
        let start = Instant::now();
        let deadline = *options.timeout.value();
        let mut rec = ExecutionRecord::new(namespace, &self.coordinator);
        rec.isolation_applied = Some(*options.isolation.value());
        rec.concurrency_applied = *options.concurrency.value();

        cancel.check()?;
        rec.set_stage(ExecutionStage::Bound);

        if let LogicalRequest::Explain { inner } = &request {
            let (lp, pp) = planner::plan((**inner).clone(), options.clone())?;
            rec.plan_id = Some(lp.id);
            rec.set_stage(ExecutionStage::Planned);
            rec.set_stage(ExecutionStage::Done);
            rec.elapsed = start.elapsed();
            let text = explain_text(&lp, &pp);
            self.push_record(rec);
            return Ok(QueryResult {
                tag: "EXPLAIN".into(),
                columns: vec!["QUERY PLAN".into()],
                rows: text.lines().map(|l| vec![Some(l.to_string())]).collect(),
            });
        }

        let (lp, pp) = planner::plan(request.clone(), options.clone())?;
        rec.plan_id = Some(lp.id);
        rec.set_stage(ExecutionStage::Planned);

        // `007`: quota admission is coordinator-local and happens before query
        // scheduling. It never contacts the cluster primary.
        let mut quota_hold: Option<(uuid::Uuid, Vec<(spacestorage_tenancy::QuotaSpec, u64)>)> =
            None;
        if let Some(tenancy) = &self.tenancy {
            let namespace_record = tenancy.registry.lock().get_by_name(namespace).cloned();
            if let Some(namespace_record) = namespace_record {
                let datatype = request_datatype(&self.catalog, namespace, &request);
                if let Some(login) = options.principal.as_deref() {
                    let roles = tenancy.roles.lock();
                    let role = roles
                        .get(login)
                        .or_else(|| roles.get("admin").filter(|_| login == "admin"));
                    if let Some(role) = role {
                        let (bytes, objects) = request_usage(&request);
                        let verb = if bytes > 0 || objects > 0 {
                            spacestorage_authz::Verb::WRITE.bits()
                        } else {
                            spacestorage_authz::Verb::READ.bits()
                        };
                        spacestorage_tenancy::authorize_consuming(
                            login,
                            role,
                            &namespace_record,
                            datatype.as_deref().unwrap_or("*"),
                            verb,
                        )
                        .map_err(|error| ExecError::Msg(error.to_string()))?;
                    }
                }
                let (bytes, objects) = request_usage(&request);
                let consuming: Vec<_> = namespace_record
                    .quotas
                    .iter()
                    .filter(|spec| spec.applies_to(datatype.as_deref()))
                    .filter_map(|spec| {
                        let amount = match spec.unit {
                            spacestorage_tenancy::QuotaUnit::Bytes if bytes > 0 => bytes,
                            spacestorage_tenancy::QuotaUnit::Objects if objects > 0 => objects,
                            spacestorage_tenancy::QuotaUnit::OpsPerSec
                                if consumes_ops(&request) =>
                            {
                                1
                            }
                            _ => return None,
                        };
                        Some((spec.clone(), amount))
                    })
                    .collect();
                spacestorage_tenancy::admit(&tenancy.usage, &namespace_record, &consuming)
                    .map_err(|error| ExecError::Msg(error.to_string()))?;
                if !consuming.is_empty() {
                    quota_hold = Some((namespace_record.id, consuming));
                }
            }
        }

        let mem = pp
            .tasks
            .iter()
            .map(|t| t.estimated_memory)
            .sum::<u64>()
            .max(1024);
        let sched = schedule::schedule(
            &self.admission,
            namespace,
            &pp,
            *options.concurrency.value(),
            mem,
            vec![],
        )?;
        let _spill_guard = if sched.token.spilled {
            if let Some(dir) = &self.data_dir {
                Some(crate::spill::SpillDir::create(dir, rec.id)?)
            } else {
                // Spill admitted without a data_dir — still honour the spill path
                // by creating under a temp sibling of CWD when needed for tests.
                let tmp = std::env::temp_dir().join("spacestorage-spill");
                Some(crate::spill::SpillDir::create(&tmp, rec.id)?)
            }
        } else {
            None
        };
        let _token = sched.token;
        if _spill_guard.is_some() {
            rec.spill_bytes = mem;
        }
        rec.set_stage(ExecutionStage::Scheduled);

        if let Some(task) = pp.tasks.first() {
            rec.set_stage(ExecutionStage::Running { task: task.id });
        }

        check_deadline(start, deadline, &cancel)?;

        let result = self.dispatch(
            namespace,
            session,
            protocol,
            request,
            options.clone(),
            &cancel,
        );

        rec.set_stage(ExecutionStage::Finalizing);
        match &result {
            Ok(_) => {
                rec.partial = *options.partial_ok.value();
                rec.set_stage(ExecutionStage::Done);
            }
            Err(ExecError::Cancelled) => rec.set_stage(ExecutionStage::Cancelled),
            Err(ExecError::Timeout) => {
                rec.set_stage(ExecutionStage::Failed);
                rec.error = Some("timeout".into());
            }
            Err(e) => {
                rec.set_stage(ExecutionStage::Failed);
                rec.error = Some(e.to_string());
            }
        }
        rec.elapsed = start.elapsed();
        if let Some(dir) = &self.data_dir {
            let _ = crate::spill::SpillDir::create(dir, rec.id);
        }
        self.push_record(rec);
        if result.is_err() {
            if let (Some(tenancy), Some((ns_id, specs))) = (&self.tenancy, quota_hold) {
                spacestorage_tenancy::admit::release(&tenancy.usage, ns_id, &specs);
            }
        }
        result
    }

    fn dispatch(
        &self,
        namespace: &str,
        session: &str,
        protocol: &str,
        request: LogicalRequest,
        options: QueryOptions,
        cancel: &CancelToken,
    ) -> Result<QueryResult, ExecError> {
        match request {
            LogicalRequest::AdHocSql { sql } => {
                cancel.check()?;
                if let Some(txn) = self.txns.get(session) {
                    if txn.state == crate::txn::TxnState::Open {
                        if txn.isolation == IsolationLevel::Snapshot {
                            self.ensure_snapshot_for_sql(namespace, session, &sql)?;
                        }
                        let verb = sql
                            .trim_start()
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .to_ascii_uppercase();
                        if matches!(verb.as_str(), "INSERT" | "UPDATE" | "DELETE") {
                            self.txns.with_mut(session, |t| {
                                t.buffered.push(crate::txn::BufferedWrite {
                                    namespace: namespace.into(),
                                    sql: sql.clone(),
                                });
                                Ok(())
                            })?;
                            return Ok(QueryResult {
                                tag: format!("{verb} 0"),
                                ..Default::default()
                            });
                        }
                    }
                }
                // SNAPSHOT reads: prefer frozen table copy when present.
                if let Some(snap) = self.snapshot_select(namespace, session, &sql) {
                    return snap;
                }
                execute_adhoc_sql(&self.catalog, namespace, &sql)
            }
            LogicalRequest::TxnBegin { isolation } => {
                self.require_distributed("BEGIN")?;
                self.txns.begin(session.into(), protocol, isolation)?;
                Ok(QueryResult {
                    tag: "BEGIN".into(),
                    ..Default::default()
                })
            }
            LogicalRequest::TxnSetIsolation { isolation } => {
                self.require_distributed("SET TRANSACTION")?;
                self.txns.with_mut(session, |txn| {
                    if txn.state != crate::txn::TxnState::Open {
                        return Err(ExecError::Msg("no_active_txn".into()));
                    }
                    txn.isolation = isolation;
                    Ok(())
                })?;
                Ok(QueryResult {
                    tag: "SET".into(),
                    ..Default::default()
                })
            }
            LogicalRequest::TxnCommit => {
                self.require_distributed("COMMIT")?;
                let mut txn = self
                    .txns
                    .take(session)
                    .ok_or_else(|| ExecError::Msg("no_active_txn".into()))?;
                txn.clear_portals();
                for w in txn.buffered.clone() {
                    execute_adhoc_sql(&self.catalog, &w.namespace, &w.sql)?;
                }
                commit_distributed(&mut txn, |_| true)?;
                Ok(QueryResult {
                    tag: "COMMIT".into(),
                    ..Default::default()
                })
            }
            LogicalRequest::TxnRollback => {
                self.require_distributed("ROLLBACK")?;
                let mut txn = self
                    .txns
                    .take(session)
                    .ok_or_else(|| ExecError::Msg("no_active_txn".into()))?;
                txn.clear_portals();
                txn.rollback()?;
                Ok(QueryResult {
                    tag: "ROLLBACK".into(),
                    ..Default::default()
                })
            }
            LogicalRequest::CopyIn {
                container,
                format,
                rows,
            } => {
                self.require_distributed("COPY")?;
                copy_in(&self.catalog, namespace, &container, format, &rows)
            }
            LogicalRequest::CopyOut {
                container, format, ..
            } => {
                self.require_distributed("COPY")?;
                copy_out(&self.catalog, namespace, &container, format)
            }
            LogicalRequest::Join {
                left,
                right,
                kind,
                left_key,
                right_key,
            } => {
                self.require_distributed("JOIN")?;
                #[cfg(feature = "query-distributed")]
                {
                    crate::engines::join::run(
                        &self.catalog,
                        namespace,
                        &left,
                        &right,
                        kind,
                        &left_key,
                        &right_key,
                    )
                }
                #[cfg(not(feature = "query-distributed"))]
                {
                    let _ = (left, right, kind, left_key, right_key);
                    Err(ExecError::NotSupported("join".into()))
                }
            }
            LogicalRequest::Aggregate {
                container,
                func,
                group_by,
                column,
                ..
            } => {
                self.require_distributed("AGGREGATE")?;
                #[cfg(feature = "query-distributed")]
                {
                    crate::engines::aggregate::run(
                        &self.catalog,
                        namespace,
                        &container,
                        func,
                        group_by.as_deref(),
                        column.as_deref(),
                    )
                }
                #[cfg(not(feature = "query-distributed"))]
                {
                    let _ = (container, func, group_by, column);
                    Err(ExecError::NotSupported("aggregate".into()))
                }
            }
            LogicalRequest::MapReduce {
                container,
                map_expr,
                reduce_expr,
            } => {
                self.require_distributed("MAPREDUCE")?;
                #[cfg(feature = "query-distributed")]
                {
                    crate::engines::mapreduce::run(
                        &self.catalog,
                        namespace,
                        &container,
                        &map_expr,
                        &reduce_expr,
                        &self.shuffle,
                    )
                }
                #[cfg(not(feature = "query-distributed"))]
                {
                    let _ = (container, map_expr, reduce_expr);
                    Err(ExecError::NotSupported("mapreduce".into()))
                }
            }
            LogicalRequest::SubscribeWait { exec_id } => {
                self.require_distributed("SUBSCRIBE")?;
                #[cfg(feature = "query-distributed")]
                {
                    let sub = crate::engines::subscribe::wait(&self.jobs, exec_id)?;
                    Ok(crate::engines::subscribe::result_from_job(&sub))
                }
                #[cfg(not(feature = "query-distributed"))]
                {
                    let _ = exec_id;
                    Err(ExecError::NotSupported("subscribe".into()))
                }
            }
            LogicalRequest::Cancel { exec_id } => {
                cancel.cancel();
                let _ = self.jobs.cancel(exec_id);
                Ok(QueryResult {
                    tag: "CANCEL".into(),
                    ..Default::default()
                })
            }
            LogicalRequest::Ddl { sql, .. } => execute_adhoc_sql(&self.catalog, namespace, &sql),
            LogicalRequest::Mutate { container, op, sql } => {
                if op.eq_ignore_ascii_case("delete") && !sql.to_ascii_uppercase().contains("FROM") {
                    // Protocol IR: key in `sql` field (Cassandra/ES/S3/WebDAV/Redis).
                    object_delete(&self.catalog, namespace, &container, &sql)
                } else {
                    execute_adhoc_sql(&self.catalog, namespace, &sql)
                }
            }
            LogicalRequest::TypeOp { container, op } => {
                let o = op.to_ascii_lowercase();
                if o.starts_with("ensure") {
                    let model = o.strip_prefix("ensure_").unwrap_or("table");
                    ensure_container(&self.catalog, namespace, &container, model)
                } else if o == "drop" {
                    execute_adhoc_sql(
                        &self.catalog,
                        namespace,
                        &format!("DROP TABLE IF EXISTS {container}"),
                    )
                } else {
                    Err(ExecError::NotSupported(format!("type_op:{op}")))
                }
            }
            LogicalRequest::Object {
                container,
                key,
                bytes,
            } => object_put(&self.catalog, namespace, &container, &key, bytes),
            LogicalRequest::Scan { container, filter } => {
                // Relational SQL scan when filter looks like a column predicate; else KV scan.
                if filter
                    .as_ref()
                    .map(|f| f.contains('=') || f.to_ascii_uppercase().starts_with("WHERE"))
                    .unwrap_or(false)
                    || protocol == "postgresql"
                {
                    let sql = match &filter {
                        Some(f) if f.to_ascii_uppercase().starts_with("WHERE") => {
                            format!("SELECT * FROM {container} {f}")
                        }
                        Some(f) if f.contains('=') => {
                            format!("SELECT * FROM {container} WHERE {f}")
                        }
                        Some(f) => format!("SELECT * FROM {container} WHERE id = '{f}'"),
                        None => format!("SELECT * FROM {container}"),
                    };
                    execute_adhoc_sql(&self.catalog, namespace, &sql)
                } else {
                    object_scan(&self.catalog, namespace, &container, filter.as_deref())
                }
            }
            LogicalRequest::Point { container, key } => {
                if protocol == "postgresql" {
                    let sql = format!("SELECT * FROM {container} WHERE id = '{key}'");
                    execute_adhoc_sql(&self.catalog, namespace, &sql)
                } else {
                    object_get(&self.catalog, namespace, &container, &key)
                }
            }
            LogicalRequest::Batch { requests } => {
                let mut last = QueryResult::default();
                for r in requests {
                    last = self.execute_blocking(
                        namespace,
                        session,
                        protocol,
                        r,
                        options.clone(),
                        cancel.clone(),
                    )?;
                }
                Ok(last)
            }
            other => Err(ExecError::NotSupported(format!("{other:?}"))),
        }
    }
}

fn consumes_ops(request: &LogicalRequest) -> bool {
    !matches!(
        request,
        LogicalRequest::Explain { .. }
            | LogicalRequest::TxnBegin { .. }
            | LogicalRequest::TxnSetIsolation { .. }
    )
}

fn request_datatype(
    catalog: &SharedCatalog,
    namespace: &str,
    request: &LogicalRequest,
) -> Option<String> {
    let container = match request {
        LogicalRequest::Object { container, .. } => container.as_str(),
        LogicalRequest::Mutate { container, .. } => container.as_str(),
        LogicalRequest::CopyIn { container, .. } => container.as_str(),
        _ => return None,
    };
    catalog
        .read()
        .ok()?
        .describe(namespace, container)
        .ok()
        .map(|d| d.model.machine_name().to_string())
}

fn request_usage(request: &LogicalRequest) -> (u64, u64) {
    match request {
        LogicalRequest::Object { bytes, .. } => (bytes.len() as u64, 1),
        LogicalRequest::Mutate { op, .. } if op.eq_ignore_ascii_case("delete") => (0, 0),
        LogicalRequest::Mutate { .. } => (0, 1),
        LogicalRequest::CopyIn { rows, .. } => {
            let bytes = rows
                .iter()
                .flat_map(|row| row.iter())
                .map(|value| value.as_ref().map(|value| value.len() as u64).unwrap_or(0))
                .sum();
            (bytes, rows.len() as u64)
        }
        LogicalRequest::AdHocSql { sql } => crate::catalog_exec::adhoc_sql_usage(sql),
        LogicalRequest::Batch { requests } => requests
            .iter()
            .map(request_usage)
            .fold((0u64, 0u64), |(ab, ao), (b, o)| {
                (ab.saturating_add(b), ao.saturating_add(o))
            }),
        _ => (0, 0),
    }
}

#[async_trait::async_trait]
impl QueryEngine for PlannerEngine {
    async fn execute(
        &self,
        namespace: &str,
        session: &str,
        protocol: &str,
        request: LogicalRequest,
        options: QueryOptions,
        cancel: CancelToken,
    ) -> Result<QueryResult, ExecError> {
        self.execute_blocking(namespace, session, protocol, request, options, cancel)
    }

    fn cancel(&self, exec_id: Uuid) -> bool {
        self.jobs.cancel(exec_id)
    }

    fn subscribe_wait(&self, exec_id: Uuid) -> Result<crate::subscribe::Subscription, ExecError> {
        self.jobs
            .get(exec_id)
            .ok_or_else(|| ExecError::Msg(format!("unknown_job:{exec_id}")))
    }
}

/// Lower SQL text to LogicalRequest for the active dialect profile.
pub fn lower_sql(
    profile: spacestorage_compat::DialectProfile,
    sql: &str,
) -> Result<LogicalRequest, ExecError> {
    use spacestorage_compat::classify_pg_verb;
    let sql = sql.trim().trim_end_matches(';');
    let verb = sql
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    let outcome = classify_pg_verb(profile, &verb);
    if !outcome.is_must() {
        return Err(ExecError::NotSupported(format!(
            "feature_not_supported: {verb}"
        )));
    }
    match verb.as_str() {
        "BEGIN" | "START" => {
            let isolation = if sql.to_ascii_uppercase().contains("SERIALIZABLE") {
                return Err(ExecError::NotSupported(
                    "SERIALIZABLE refused; allowed: READ COMMITTED, SNAPSHOT".into(),
                ));
            } else if sql.to_ascii_uppercase().contains("SNAPSHOT")
                || sql.to_ascii_uppercase().contains("REPEATABLE READ")
            {
                IsolationLevel::Snapshot
            } else {
                IsolationLevel::ReadCommitted
            };
            Ok(LogicalRequest::TxnBegin { isolation })
        }
        "COMMIT" => Ok(LogicalRequest::TxnCommit),
        "ROLLBACK" => Ok(LogicalRequest::TxnRollback),
        "COPY" => lower_copy(profile, sql),
        "SET" => {
            if let Some(isolation) = parse_set_transaction_isolation(sql)? {
                return Ok(LogicalRequest::TxnSetIsolation { isolation });
            }
            Ok(LogicalRequest::AdHocSql {
                sql: sql.to_string(),
            })
        }
        "EXPLAIN" => {
            let inner_sql = sql["EXPLAIN".len()..].trim();
            let inner = lower_sql(profile, inner_sql)?;
            Ok(LogicalRequest::Explain {
                inner: Box::new(inner),
            })
        }
        "DECLARE" | "FETCH" | "CLOSE" => {
            // Forward-only cursors: represented as AdHoc for session portal in handler;
            // IR is AdHocSql so planner records stages.
            Ok(LogicalRequest::AdHocSql {
                sql: sql.to_string(),
            })
        }
        _ => Ok(LogicalRequest::AdHocSql {
            sql: sql.to_string(),
        }),
    }
}

fn refuse_pg_copy(outcome: ClassifyOutcome) -> ExecError {
    match outcome {
        ClassifyOutcome::MustNot(err) => {
            ExecError::NotSupported(format!("feature_not_supported: COPY ({})", err.code()))
        }
        ClassifyOutcome::Must => ExecError::NotSupported("feature_not_supported: COPY".into()),
    }
}

fn lower_copy(profile: DialectProfile, sql: &str) -> Result<LogicalRequest, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let program = upper.contains("PROGRAM");
    let freeze = upper.contains("FREEZE");
    let format_label = if upper.contains("BINARY") {
        Some("binary")
    } else if upper.contains("CSV") {
        Some("csv")
    } else {
        Some("text")
    };
    let format = if upper.contains("BINARY") {
        crate::request::CopyFormat::Binary
    } else if upper.contains("CSV") {
        crate::request::CopyFormat::Csv
    } else {
        crate::request::CopyFormat::Text
    };
    let server_path = copy_target_is_server_path(&upper);
    let gate =
        spacestorage_compat::classify_pg_copy(profile, format_label, program, freeze, server_path);
    if !gate.is_must() {
        return Err(refuse_pg_copy(gate));
    }
    // COPY name FROM STDIN / TO STDOUT
    if upper.contains(" FROM ") {
        let name = extract_ident_after(&upper, sql, "COPY")?;
        Ok(LogicalRequest::CopyIn {
            container: name,
            format,
            rows: vec![],
        })
    } else if upper.contains(" TO ") {
        let name = extract_ident_after(&upper, sql, "COPY")?;
        Ok(LogicalRequest::CopyOut {
            container: name,
            format,
            filter: None,
        })
    } else {
        Err(ExecError::Msg("COPY requires FROM or TO".into()))
    }
}

fn copy_target_is_server_path(upper: &str) -> bool {
    let marker = if upper.contains(" FROM ") {
        " FROM "
    } else if upper.contains(" TO ") {
        " TO "
    } else {
        return false;
    };
    let Some(pos) = upper.find(marker) else {
        return false;
    };
    let target = upper[pos + marker.len()..].trim();
    let head = target
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches('"')
        .trim_matches('\'');
    !head.is_empty() && head != "STDIN" && head != "STDOUT" && !head.starts_with("(")
}

fn extract_table_from_dml(upper: &str, original: &str) -> Option<String> {
    for kw in [" FROM ", " INTO ", " UPDATE ", " TABLE "] {
        if let Some(pos) = upper.find(kw) {
            let rest = original[pos + kw.len()..].trim_start();
            let name = rest
                .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
                .next()
                .unwrap_or("")
                .trim_matches('"');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn extract_ident_after(upper: &str, original: &str, keyword: &str) -> Result<String, ExecError> {
    let pos = upper
        .find(keyword)
        .ok_or_else(|| ExecError::Msg(format!("missing {keyword}")))?;
    let rest = original[pos + keyword.len()..].trim_start();
    Ok(rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_types::ContainerCatalog;
    use std::sync::RwLock;

    #[cfg(feature = "tenancy-quotas")]
    #[test]
    fn tenancy_admit_rejects_before_scheduled() {
        let tenancy = Arc::new(spacestorage_tenancy::Tenancy::new());
        let namespace = tenancy
            .registry
            .lock()
            .create(spacestorage_authz::Verb::CLUSTER_ADMIN.bits(), "acme")
            .unwrap();
        tenancy
            .registry
            .lock()
            .replace_quotas(
                spacestorage_authz::Verb::CLUSTER_ADMIN.bits(),
                namespace.id,
                vec![spacestorage_tenancy::QuotaSpec {
                    unit: spacestorage_tenancy::QuotaUnit::Bytes,
                    limit: 0,
                    datatype: None,
                }],
            )
            .unwrap();
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let engine = PlannerEngine::new(catalog).with_tenancy(tenancy);
        let error = engine
            .execute_blocking(
                "acme",
                "s",
                "postgresql",
                LogicalRequest::Object {
                    container: "items".into(),
                    key: "k".into(),
                    bytes: vec![1],
                },
                QueryOptions::default(),
                CancelToken::new(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("QuotaExceeded"), "{error}");
        assert!(
            engine
                .records_snapshot()
                .last()
                .map(|record| record.stage != ExecutionStage::Scheduled)
                .unwrap_or(true)
        );
    }

    #[cfg(feature = "tenancy-quotas")]
    #[test]
    fn adhoc_insert_admits_bytes_and_objects_quota() {
        let tenancy = Arc::new(spacestorage_tenancy::Tenancy::new());
        let namespace = tenancy
            .registry
            .lock()
            .create(spacestorage_authz::Verb::CLUSTER_ADMIN.bits(), "acme")
            .unwrap();
        tenancy
            .registry
            .lock()
            .replace_quotas(
                spacestorage_authz::Verb::CLUSTER_ADMIN.bits(),
                namespace.id,
                vec![spacestorage_tenancy::QuotaSpec {
                    unit: spacestorage_tenancy::QuotaUnit::Objects,
                    limit: 1,
                    datatype: None,
                }],
            )
            .unwrap();
        tenancy.usage.set(
            namespace.id,
            spacestorage_tenancy::QuotaUnit::Objects,
            &None,
            1,
        );
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let engine = PlannerEngine::new(catalog).with_tenancy(tenancy);
        let sql = "INSERT INTO t VALUES ('1', 'a')";
        let err = engine
            .execute_blocking(
                "acme",
                "s",
                "postgresql",
                LogicalRequest::AdHocSql { sql: sql.into() },
                QueryOptions::default(),
                CancelToken::new(),
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("QuotaExceeded"),
            "insert must consume objects: {err}"
        );
        assert!(err.to_string().contains("limit=1"), "{err}");
        assert!(err.to_string().contains("usage=1"), "{err}");
    }

    #[cfg(feature = "tenancy-quotas")]
    #[test]
    fn adhoc_select_and_delete_skip_bytes_objects_quota() {
        let tenancy = Arc::new(spacestorage_tenancy::Tenancy::new());
        let namespace = tenancy
            .registry
            .lock()
            .create(spacestorage_authz::Verb::CLUSTER_ADMIN.bits(), "acme")
            .unwrap();
        tenancy
            .registry
            .lock()
            .replace_quotas(
                spacestorage_authz::Verb::CLUSTER_ADMIN.bits(),
                namespace.id,
                vec![spacestorage_tenancy::QuotaSpec {
                    unit: spacestorage_tenancy::QuotaUnit::Objects,
                    limit: 0,
                    datatype: None,
                }],
            )
            .unwrap();
        tenancy.usage.set(
            namespace.id,
            spacestorage_tenancy::QuotaUnit::Objects,
            &None,
            0,
        );
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let engine = PlannerEngine::with_distributed(catalog.clone(), true).with_tenancy(tenancy);
        for sql in ["SELECT 1", "DELETE FROM t WHERE id = '1'"] {
            let outcome = engine.execute_blocking(
                "acme",
                "s",
                "postgresql",
                LogicalRequest::AdHocSql { sql: sql.into() },
                QueryOptions::default(),
                CancelToken::new(),
            );
            if let Err(err) = outcome {
                assert!(
                    !err.to_string().contains("QuotaExceeded"),
                    "{sql} must not admit objects: {err}"
                );
            }
        }
    }

    #[cfg(feature = "tenancy-quotas")]
    #[test]
    fn delete_mutate_does_not_admit_objects_quota() {
        let tenancy = Arc::new(spacestorage_tenancy::Tenancy::new());
        let namespace = tenancy
            .registry
            .lock()
            .create(spacestorage_authz::Verb::CLUSTER_ADMIN.bits(), "acme")
            .unwrap();
        tenancy
            .registry
            .lock()
            .replace_quotas(
                spacestorage_authz::Verb::CLUSTER_ADMIN.bits(),
                namespace.id,
                vec![spacestorage_tenancy::QuotaSpec {
                    unit: spacestorage_tenancy::QuotaUnit::Objects,
                    limit: 1,
                    datatype: None,
                }],
            )
            .unwrap();
        tenancy.usage.set(
            namespace.id,
            spacestorage_tenancy::QuotaUnit::Objects,
            &None,
            1,
        );
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let engine = PlannerEngine::new(catalog).with_tenancy(tenancy);
        let err = engine
            .execute_blocking(
                "acme",
                "s",
                "cassandra",
                LogicalRequest::Mutate {
                    container: "t".into(),
                    op: "delete".into(),
                    sql: "k".into(),
                },
                QueryOptions::default(),
                CancelToken::new(),
            )
            .unwrap_err();
        assert!(
            !err.to_string().contains("QuotaExceeded"),
            "delete must not consume objects quota: {err}"
        );
    }

    #[tokio::test]
    async fn engine_label_planner() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let eng = PlannerEngine::new(cat);
        let _ = eng
            .execute(
                "ns",
                "s",
                "postgresql",
                LogicalRequest::AdHocSql {
                    sql: "SELECT 1".into(),
                },
                QueryOptions::default(),
                CancelToken::new(),
            )
            .await
            .unwrap();
        let recs = eng.records_snapshot();
        assert_eq!(recs.last().unwrap().engine, "planner");
        assert_eq!(recs.last().unwrap().stage, ExecutionStage::Done);
    }

    #[tokio::test]
    async fn begin_refused_without_distributed() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let eng = PlannerEngine::new(cat);
        let err = eng
            .execute(
                "ns",
                "s",
                "postgresql",
                LogicalRequest::TxnBegin {
                    isolation: IsolationLevel::ReadCommitted,
                },
                QueryOptions::default(),
                CancelToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code(), "not_supported");
    }

    #[tokio::test]
    async fn begin_commit_with_distributed() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let eng = PlannerEngine::with_distributed(cat.clone(), true);
        eng.execute(
            "ns",
            "s",
            "postgresql",
            LogicalRequest::TxnBegin {
                isolation: IsolationLevel::ReadCommitted,
            },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .await
        .unwrap();
        eng.execute(
            "ns",
            "s",
            "postgresql",
            LogicalRequest::AdHocSql {
                sql: "CREATE TABLE t (id text, v text)".into(),
            },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .await
        .unwrap();
        eng.execute(
            "ns",
            "s",
            "postgresql",
            LogicalRequest::AdHocSql {
                sql: "INSERT INTO t VALUES ('1', 'a')".into(),
            },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .await
        .unwrap();
        eng.execute(
            "ns",
            "s",
            "postgresql",
            LogicalRequest::TxnCommit,
            QueryOptions::default(),
            CancelToken::new(),
        )
        .await
        .unwrap();
        let sel = eng
            .execute(
                "ns",
                "s2",
                "postgresql",
                LogicalRequest::AdHocSql {
                    sql: "SELECT * FROM t".into(),
                },
                QueryOptions::default(),
                CancelToken::new(),
            )
            .await
            .unwrap();
        assert!(!sel.rows.is_empty());
    }

    #[test]
    fn lower_set_transaction_maps_to_txn_set_isolation() {
        use spacestorage_compat::DialectProfile;
        let req = lower_sql(
            DialectProfile::CompleteProduct,
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        )
        .unwrap();
        assert!(matches!(
            req,
            LogicalRequest::TxnSetIsolation {
                isolation: IsolationLevel::Snapshot
            }
        ));
    }

    #[tokio::test]
    async fn set_transaction_updates_open_txn_isolation() {
        use spacestorage_compat::DialectProfile;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let eng = PlannerEngine::with_distributed(cat, true);
        let session = "iso-t078";
        eng.execute(
            "ns",
            session,
            "postgresql",
            LogicalRequest::TxnBegin {
                isolation: IsolationLevel::ReadCommitted,
            },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .await
        .unwrap();
        let req = lower_sql(
            DialectProfile::CompleteProduct,
            "SET TRANSACTION ISOLATION LEVEL SNAPSHOT",
        )
        .unwrap();
        eng.execute(
            "ns",
            session,
            "postgresql",
            req,
            QueryOptions::default(),
            CancelToken::new(),
        )
        .await
        .unwrap();
        let txn = eng.txns().get(session).expect("open txn");
        assert_eq!(txn.isolation, IsolationLevel::Snapshot);
    }

    #[test]
    fn copy_program_refused_before_copy_ir() {
        use spacestorage_compat::DialectProfile;
        let err = lower_copy(
            DialectProfile::CompleteProduct,
            "COPY t FROM PROGRAM 'echo hi'",
        )
        .unwrap_err();
        assert!(err.to_string().contains("feature_not_supported: COPY"));
    }

    #[test]
    fn copy_server_path_refused() {
        use spacestorage_compat::DialectProfile;
        let err = lower_copy(
            DialectProfile::CompleteProduct,
            "COPY t FROM '/tmp/data.csv'",
        )
        .unwrap_err();
        assert!(err.to_string().contains("feature_not_supported: COPY"));
    }
}
