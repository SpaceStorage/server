//! PlannerEngine — default production QueryEngine (005).

use crate::admission::{AdmissionController, AdmissionLimits};
use crate::cancel::{check_deadline, CancelToken};
use crate::catalog_exec::{
    copy_in, copy_out, ensure_container, execute_adhoc_sql, object_delete, object_get, object_put,
    object_scan, QueryResult, SharedCatalog,
};
use crate::error::ExecError;
use crate::options::{IsolationLevel, QueryOptions};
use crate::planner::{self, explain_text};
use crate::record::{ExecutionRecord, ExecutionStage};
use crate::request::LogicalRequest;
use crate::schedule;
use crate::subscribe::{JobRegistry, SharedJobRegistry};
use crate::txn::{commit_distributed, SharedTxnRegistry, TxnRegistry};
use parking_lot::Mutex;
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

    fn explain(
        &self,
        request: LogicalRequest,
        options: QueryOptions,
    ) -> Result<String, ExecError> {
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
}

impl PlannerEngine {
    pub fn new(catalog: SharedCatalog) -> Self {
        Self::with_limits(catalog, AdmissionLimits::default(), false)
    }

    pub fn with_distributed(catalog: SharedCatalog, distributed: bool) -> Self {
        Self::with_limits(catalog, AdmissionLimits::default(), distributed)
    }

    pub fn with_limits(
        catalog: SharedCatalog,
        limits: AdmissionLimits,
        distributed: bool,
    ) -> Self {
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
        }
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
        let _token = sched.token;
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
                if let Some(txn) = self.txns.get(session) {
                    if txn.state == crate::txn::TxnState::Open {
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
            LogicalRequest::TxnCommit => {
                self.require_distributed("COMMIT")?;
                let mut txn = self
                    .txns
                    .take(session)
                    .ok_or_else(|| ExecError::Msg("no_active_txn".into()))?;
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
                container,
                format,
                ..
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
                    object_scan(
                        &self.catalog,
                        namespace,
                        &container,
                        filter.as_deref(),
                    )
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
        "COPY" => lower_copy(sql),
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

fn lower_copy(sql: &str) -> Result<LogicalRequest, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let format = if upper.contains("BINARY") {
        crate::request::CopyFormat::Binary
    } else if upper.contains("CSV") {
        crate::request::CopyFormat::Csv
    } else {
        crate::request::CopyFormat::Text
    };
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
}
