//! Canonical IR — `LogicalRequest` (005 / 002 seam).

use crate::options::IsolationLevel;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type ExecId = Uuid;
pub type ContainerRef = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyFormat {
    Text,
    Csv,
    Binary,
}

impl CopyFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "TEXT" => Some(Self::Text),
            "CSV" => Some(Self::Csv),
            "BINARY" => Some(Self::Binary),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinKind {
    Inner,
    Left,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggFn {
    Count,
    Sum,
    Min,
    Max,
    Avg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LogicalRequest {
    Ddl {
        sql: String,
        container: Option<ContainerRef>,
    },
    Point {
        container: ContainerRef,
        key: String,
    },
    Scan {
        container: ContainerRef,
        filter: Option<String>,
    },
    Mutate {
        container: ContainerRef,
        op: String,
        sql: String,
    },
    TypeOp {
        container: ContainerRef,
        op: String,
    },
    Object {
        container: ContainerRef,
        key: String,
        bytes: Vec<u8>,
    },
    Batch {
        requests: Vec<LogicalRequest>,
    },
    Aggregate {
        container: ContainerRef,
        func: AggFn,
        group_by: Option<String>,
        column: Option<String>,
        filter: Option<String>,
    },
    Join {
        left: ContainerRef,
        right: ContainerRef,
        kind: JoinKind,
        left_key: String,
        right_key: String,
    },
    Explain {
        inner: Box<LogicalRequest>,
    },
    CopyIn {
        container: ContainerRef,
        format: CopyFormat,
        rows: Vec<Vec<Option<String>>>,
    },
    CopyOut {
        container: ContainerRef,
        format: CopyFormat,
        filter: Option<String>,
    },
    TxnBegin {
        isolation: IsolationLevel,
    },
    TxnCommit,
    TxnRollback,
    SubscribeWait {
        exec_id: ExecId,
    },
    Cancel {
        exec_id: ExecId,
    },
    /// Opaque SQL / protocol text for planner lowering (handlers MAY pass this).
    AdHocSql {
        sql: String,
    },
    MapReduce {
        container: ContainerRef,
        map_expr: String,
        reduce_expr: String,
    },
}
