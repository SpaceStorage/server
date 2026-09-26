//! Logical → physical plan + type catalog refuse (005).

use crate::error::ExecError;
use crate::options::{Concurrency, IsolationLevel, QueryOptions};
use crate::record::{PlanId, TaskId};
use crate::request::{JoinKind, LogicalRequest};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineKind {
    Scan,
    Point,
    Mutate,
    NestedLoopJoin,
    HashJoin,
    Aggregate,
    Map,
    Reduce,
    Shuffle,
    Copy,
    Ddl,
    Explain,
    Txn,
    Job,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub engine: EngineKind,
    pub placement: Vec<String>,
    pub shard: Option<String>,
    pub estimated_memory: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalPlan {
    pub id: PlanId,
    pub root: LogicalRequest,
    pub containers: Vec<String>,
    pub ops: Vec<String>,
    pub isolation: IsolationLevel,
    pub concurrency: Concurrency,
    pub options: QueryOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalPlan {
    pub logical: PlanId,
    pub tasks: Vec<Task>,
    pub edges: Vec<(TaskId, TaskId)>,
}

/// Operations known for RelationalTable / KV (003 subset used by planner).
pub fn operations_for_type(ty: &str) -> &'static [&'static str] {
    match ty {
        "RelationalTable" | "relational_table" => &[
            "insert", "select", "update", "delete", "create", "drop", "scan", "copy",
        ],
        "KvStore" | "kv_store" | "K/V Store" => &["get", "set", "del", "exists", "scan"],
        "DocumentStore" | "document_store" => &["put", "get", "delete", "search"],
        _ => &[],
    }
}

pub fn refuse_unsupported(container: &str, ty: &str, op: &str) -> Result<(), ExecError> {
    let ops = operations_for_type(ty);
    if ops.iter().any(|o| o.eq_ignore_ascii_case(op)) {
        Ok(())
    } else {
        Err(ExecError::UnsupportedByType {
            container: container.into(),
            ty: ty.into(),
            op: op.into(),
        })
    }
}

pub fn plan(request: LogicalRequest, options: QueryOptions) -> Result<(LogicalPlan, PhysicalPlan), ExecError> {
    let id = Uuid::now_v7();
    let (containers, ops, tasks) = analyze(&request)?;
    let logical = LogicalPlan {
        id,
        root: request,
        containers,
        ops,
        isolation: *options.isolation.value(),
        concurrency: *options.concurrency.value(),
        options,
    };
    let physical = PhysicalPlan {
        logical: id,
        tasks,
        edges: vec![],
    };
    Ok((logical, physical))
}

fn analyze(req: &LogicalRequest) -> Result<(Vec<String>, Vec<String>, Vec<Task>), ExecError> {
    let mut containers = Vec::new();
    let mut ops = Vec::new();
    let mut tasks = Vec::new();

    match req {
        LogicalRequest::Explain { inner } => {
            let (c, o, _) = analyze(inner)?;
            containers = c;
            ops = o;
            tasks.push(Task {
                id: Uuid::now_v7(),
                engine: EngineKind::Explain,
                placement: vec![],
                shard: None,
                estimated_memory: 0,
            });
        }
        LogicalRequest::Ddl { container, .. } => {
            if let Some(c) = container {
                containers.push(c.clone());
            }
            ops.push("ddl".into());
            tasks.push(simple(EngineKind::Ddl));
        }
        LogicalRequest::Point { container, .. } => {
            containers.push(container.clone());
            ops.push("point".into());
            tasks.push(simple(EngineKind::Point));
        }
        LogicalRequest::Scan { container, .. } | LogicalRequest::CopyOut { container, .. } => {
            containers.push(container.clone());
            ops.push("scan".into());
            tasks.push(simple(EngineKind::Scan));
        }
        LogicalRequest::Mutate { container, op, .. } => {
            containers.push(container.clone());
            ops.push(op.clone());
            tasks.push(simple(EngineKind::Mutate));
        }
        LogicalRequest::TypeOp { container, op } => {
            containers.push(container.clone());
            ops.push(op.clone());
            tasks.push(simple(EngineKind::Mutate));
        }
        LogicalRequest::Object { container, .. } => {
            containers.push(container.clone());
            ops.push("object".into());
            tasks.push(simple(EngineKind::Mutate));
        }
        LogicalRequest::Batch { requests } => {
            for r in requests {
                let (c, o, t) = analyze(r)?;
                containers.extend(c);
                ops.extend(o);
                tasks.extend(t);
            }
        }
        LogicalRequest::Aggregate { container, func, .. } => {
            containers.push(container.clone());
            ops.push(format!("agg:{func:?}"));
            tasks.push(simple(EngineKind::Aggregate));
        }
        LogicalRequest::Join {
            left,
            right,
            kind,
            ..
        } => {
            containers.push(left.clone());
            containers.push(right.clone());
            ops.push(format!("join:{kind:?}"));
            let engine = match kind {
                JoinKind::Inner | JoinKind::Left => EngineKind::HashJoin,
            };
            tasks.push(simple(engine));
        }
        LogicalRequest::CopyIn { container, .. } => {
            containers.push(container.clone());
            ops.push("copy_in".into());
            tasks.push(simple(EngineKind::Copy));
        }
        LogicalRequest::TxnBegin { .. }
        | LogicalRequest::TxnCommit
        | LogicalRequest::TxnRollback => {
            ops.push("txn".into());
            tasks.push(simple(EngineKind::Txn));
        }
        LogicalRequest::SubscribeWait { .. } | LogicalRequest::Cancel { .. } => {
            ops.push("job".into());
            tasks.push(simple(EngineKind::Job));
        }
        LogicalRequest::AdHocSql { .. } => {
            ops.push("adhoc".into());
            tasks.push(simple(EngineKind::Scan));
        }
        LogicalRequest::MapReduce { container, .. } => {
            containers.push(container.clone());
            ops.push("mapreduce".into());
            tasks.push(simple(EngineKind::Map));
            tasks.push(simple(EngineKind::Shuffle));
            tasks.push(simple(EngineKind::Reduce));
        }
    }
    Ok((containers, ops, tasks))
}

fn simple(engine: EngineKind) -> Task {
    Task {
        id: Uuid::now_v7(),
        engine,
        placement: vec!["local".into()],
        shard: None,
        estimated_memory: 64 * 1024,
    }
}

pub fn explain_text(plan: &LogicalPlan, physical: &PhysicalPlan) -> String {
    let mut out = String::from("LogicalPlan\n");
    out.push_str(&format!("  plan_id: {}\n", plan.id));
    out.push_str(&format!("  containers: {:?}\n", plan.containers));
    out.push_str(&format!("  ops: {:?}\n", plan.ops));
    out.push_str(&format!("  isolation: {}\n", plan.isolation.as_str()));
    out.push_str("PhysicalPlan\n");
    for t in &physical.tasks {
        out.push_str(&format!("  task {:?} engine={:?}\n", t.id, t.engine));
    }
    out
}

/// FIRST-BINARY: COPY/BEGIN must never become LogicalRequest (handler gate).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_refuse_before_mutate() {
        let err = refuse_unsupported("t", "RelationalTable", "listen").unwrap_err();
        assert_eq!(err.code(), "unsupported_by_type");
    }

    #[test]
    fn explain_scan_plan() {
        let (lp, pp) = plan(
            LogicalRequest::Explain {
                inner: Box::new(LogicalRequest::Scan {
                    container: "t".into(),
                    filter: None,
                }),
            },
            QueryOptions::default(),
        )
        .unwrap();
        assert_eq!(lp.containers, vec!["t"]);
        assert!(pp.tasks.iter().any(|t| t.engine == EngineKind::Explain));
        let text = explain_text(&lp, &pp);
        assert!(text.contains("Scan") || text.contains("t"));
    }
}
