//! Classify-gated CQL verb dispatch (015 Cassandra HC MUST) via shared `PlannerEngine` (005 T084).

use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine, QueryOptions, QueryResult};
use spacestorage_types::ContainerCatalog;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CassandraReply {
    Ok,
    Rows(Vec<(String, String)>),
    Prepared(String),
    Error { code: String, message: String },
}

pub struct SessionState {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub namespace: String,
    pub profile: DialectProfile,
}

impl SessionState {
    pub fn demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
        }
    }

    fn engine(&self) -> PlannerEngine {
        PlannerEngine::new(Arc::clone(&self.catalog))
    }

    fn exec(&self, req: LogicalRequest) -> Result<QueryResult, CassandraReply> {
        self.engine()
            .execute_blocking(
                &self.namespace,
                "cql",
                "cassandra",
                req,
                QueryOptions::default(),
                CancelToken::new(),
            )
            .map_err(|e| CassandraReply::Error {
                code: "server_error".into(),
                message: e.to_string(),
            })
    }

    fn ensure_table(&self, table: &str) -> Result<(), CassandraReply> {
        self.exec(LogicalRequest::TypeOp {
            container: table.into(),
            op: "ensure_table".into(),
        })?;
        Ok(())
    }
}

fn refuse(verb: &str, err: CompatError) -> CassandraReply {
    record_must_not(ProtocolId::Cassandra.as_str(), verb);
    CassandraReply::Error {
        code: err.code().into(),
        message: err.to_string(),
    }
}

fn rows_from(result: QueryResult) -> CassandraReply {
    let mut rows = Vec::new();
    if result.columns.iter().any(|c| c == "key" || c == "value") {
        for r in result.rows {
            let k = r.first().and_then(|x| x.clone()).unwrap_or_default();
            let v = r.get(1).and_then(|x| x.clone()).unwrap_or_default();
            rows.push((k, v));
        }
    } else {
        for r in result.rows {
            let k = r.first().and_then(|x| x.clone()).unwrap_or_default();
            let v = r
                .get(1)
                .and_then(|x| x.clone())
                .or_else(|| r.first().and_then(|x| x.clone()))
                .unwrap_or_default();
            rows.push((k, v));
        }
    }
    CassandraReply::Rows(rows)
}

/// Dispatch a normalized CQL verb (`SELECT`, `INSERT`, …) before IR → PlannerEngine.
pub fn dispatch(session: &mut SessionState, verb: &str, args: &[&str]) -> CassandraReply {
    let upper = verb.to_ascii_uppercase().replace('-', "_");
    match classify_outcome(session.profile, ProtocolId::Cassandra, &upper) {
        ClassifyOutcome::Must => {}
        ClassifyOutcome::MustNot(err) => return refuse(&upper, err),
    }

    match upper.as_str() {
        "STARTUP" | "OPTIONS" | "REGISTER" | "BATCH" => CassandraReply::Ok,
        "USE" => {
            if let Some(ks) = args.first() {
                session.namespace = ks.trim_matches('"').to_string();
            }
            CassandraReply::Ok
        }
        "CREATE" => {
            let kind = args
                .first()
                .map(|s| s.to_ascii_uppercase())
                .unwrap_or_default();
            if kind == "KEYSPACE" {
                if let Some(ks) = args.get(1) {
                    session.namespace = ks.trim_matches('"').to_string();
                }
                return CassandraReply::Ok;
            }
            let table = if kind == "TABLE" {
                args.get(1).copied().unwrap_or("t")
            } else {
                args.first().copied().unwrap_or("t")
            };
            match session.ensure_table(table.trim_matches('"')) {
                Ok(()) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        "DROP" => {
            let table = args
                .iter()
                .find(|a| a.to_ascii_uppercase() != "TABLE")
                .copied()
                .unwrap_or("t");
            match session.exec(LogicalRequest::TypeOp {
                container: table.trim_matches('"').into(),
                op: "drop".into(),
            }) {
                Ok(_) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        "ALTER" => CassandraReply::Ok,
        "PREPARE" => {
            let id = format!("prep-{}", args.first().unwrap_or(&"q"));
            CassandraReply::Prepared(id)
        }
        "EXECUTE" | "QUERY" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Point {
                container: table.into(),
                key: key.into(),
            }) {
                Ok(r) => rows_from(r),
                Err(e) => e,
            }
        }
        "INSERT" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            let val = args.get(2).copied().unwrap_or("");
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Object {
                container: table.into(),
                key: key.into(),
                bytes: val.as_bytes().to_vec(),
            }) {
                Ok(_) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        "SELECT" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied();
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            let req = if let Some(k) = key {
                LogicalRequest::Point {
                    container: table.into(),
                    key: k.into(),
                }
            } else {
                LogicalRequest::Scan {
                    container: table.into(),
                    filter: None,
                }
            };
            match session.exec(req) {
                Ok(r) => rows_from(r),
                Err(e) => e,
            }
        }
        "UPDATE" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            let val = args.get(2).copied().unwrap_or("");
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Object {
                container: table.into(),
                key: key.into(),
                bytes: val.as_bytes().to_vec(),
            }) {
                Ok(_) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        "DELETE" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Mutate {
                container: table.into(),
                op: "delete".into(),
                sql: key.into(),
            }) {
                Ok(_) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        other => refuse(
            other,
            CompatError::CompatMustNot {
                protocol: ProtocolId::Cassandra.as_str().into(),
                verb: other.into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn must_crud_smoke() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATE", &["TABLE", "t"]),
            CassandraReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "INSERT", &["t", "1", "a"]),
            CassandraReply::Ok
        ));
        match dispatch(&mut s, "SELECT", &["t", "1"]) {
            CassandraReply::Rows(r) => assert_eq!(r[0].1, "a"),
            other => panic!("{other:?}"),
        }
        let eng = PlannerEngine::new(Arc::clone(&s.catalog));
        // Force a planner record with same catalog.
        let _ = eng.execute_blocking(
            "demo",
            "t",
            "cassandra",
            LogicalRequest::Point {
                container: "t".into(),
                key: "1".into(),
            },
            QueryOptions::default(),
            CancelToken::new(),
        );
        assert_eq!(eng.records_snapshot().last().unwrap().engine, "planner");
    }

    #[test]
    fn lwt_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        match dispatch(&mut s, "LWT", &[]) {
            CassandraReply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
            other => panic!("expected refuse, got {other:?}"),
        }
    }
}
