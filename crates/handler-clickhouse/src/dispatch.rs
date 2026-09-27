//! Classify-gated ClickHouse SQL verbs via shared `PlannerEngine` (005 T085).

use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine, QueryOptions, QueryResult};
use spacestorage_types::ContainerCatalog;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickHouseReply {
    Ok,
    Rows(Vec<(String, String)>),
    Error { code: String, message: String },
}

pub struct SessionState {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub namespace: String,
    pub profile: DialectProfile,
    pub protocol: ProtocolId,
}

impl SessionState {
    pub fn demo_native(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            protocol: ProtocolId::ClickHouse,
        }
    }

    pub fn demo_http(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            protocol: ProtocolId::ClickHouseHttp,
        }
    }

    fn engine(&self) -> PlannerEngine {
        PlannerEngine::new(Arc::clone(&self.catalog))
    }

    fn exec(&self, req: LogicalRequest) -> Result<QueryResult, ClickHouseReply> {
        self.engine()
            .execute_blocking(
                &self.namespace,
                "ch",
                "clickhouse",
                req,
                QueryOptions::default(),
                CancelToken::new(),
            )
            .map_err(|e| ClickHouseReply::Error {
                code: "server_error".into(),
                message: e.to_string(),
            })
    }

    fn ensure_table(&self, table: &str) -> Result<(), ClickHouseReply> {
        self.exec(LogicalRequest::TypeOp {
            container: table.into(),
            op: "ensure_table".into(),
        })?;
        Ok(())
    }
}

fn refuse(protocol: ProtocolId, verb: &str, err: CompatError) -> ClickHouseReply {
    record_must_not(protocol.as_str(), verb);
    ClickHouseReply::Error {
        code: err.code().into(),
        message: err.to_string(),
    }
}

fn rows_from(result: QueryResult) -> ClickHouseReply {
    let mut rows = Vec::new();
    for r in result.rows {
        let k = r.first().and_then(|x| x.clone()).unwrap_or_default();
        let v = r.get(1).and_then(|x| x.clone()).unwrap_or_default();
        rows.push((k, v));
    }
    ClickHouseReply::Rows(rows)
}

pub fn dispatch(session: &mut SessionState, verb: &str, args: &[&str]) -> ClickHouseReply {
    let upper = verb.to_ascii_uppercase().replace('-', "_");
    match classify_outcome(session.profile, session.protocol, &upper) {
        ClassifyOutcome::Must => {}
        ClassifyOutcome::MustNot(err) => return refuse(session.protocol, &upper, err),
    }

    match upper.as_str() {
        "CREATE" => {
            let table = args
                .iter()
                .find(|a| {
                    !matches!(
                        a.to_ascii_uppercase().as_str(),
                        "TABLE" | "IF" | "NOT" | "EXISTS"
                    )
                })
                .copied()
                .unwrap_or("t");
            match session.ensure_table(table) {
                Ok(()) => ClickHouseReply::Ok,
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
                Ok(_) => ClickHouseReply::Ok,
                Err(e) => e,
            }
        }
        "SELECT" | "SELECT_WHERE" => {
            let table = args.first().copied().unwrap_or("t");
            let filter = args.get(1).map(|s| (*s).to_string());
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Scan {
                container: table.into(),
                filter,
            }) {
                Ok(r) => rows_from(r),
                Err(e) => e,
            }
        }
        other => refuse(
            session.protocol,
            other,
            CompatError::CompatMustNot {
                protocol: session.protocol.as_str().into(),
                verb: other.into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_where_smoke() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo_native(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATE", &["TABLE", "t"]),
            ClickHouseReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "INSERT", &["t", "1", "x"]),
            ClickHouseReply::Ok
        ));
        match dispatch(&mut s, "SELECT_WHERE", &["t", "1"]) {
            ClickHouseReply::Rows(r) => assert_eq!(r.len(), 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn group_by_and_dictionary_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo_http(cat);
        for v in ["GROUP_BY", "DICTIONARY"] {
            match dispatch(&mut s, v, &[]) {
                ClickHouseReply::Error { .. } => {}
                other => panic!("{v} must refuse, got {other:?}"),
            }
        }
    }
}
