//! Classify-gated CQL verb dispatch (015 Cassandra HC MUST).

use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_types::{
    ContainerCatalog, ContainerId, ContainerSchema, Field, L3Model, StorageModeChoice, TypeError,
    ValueDomain,
};

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

    fn ensure_table(&self, table: &str) -> Result<ContainerId, CassandraReply> {
        let mut cat = self.catalog.write().expect("catalog");
        match cat.describe(&self.namespace, table) {
            Ok(c) => {
                if c.model != L3Model::RelationalTable {
                    return Err(CassandraReply::Error {
                        code: "compat_must_not".into(),
                        message: "wrong type for CQL table".into(),
                    });
                }
                Ok(c.id)
            }
            Err(TypeError::NotFound) => {
                let schema = ContainerSchema {
                    fields: vec![
                        Field {
                            name: "id".into(),
                            domain: ValueDomain::Utf8,
                            nullable: false,
                        },
                        Field {
                            name: "v".into(),
                            domain: ValueDomain::Utf8,
                            nullable: true,
                        },
                    ],
                };
                cat.create(
                    &self.namespace,
                    table,
                    L3Model::RelationalTable,
                    false,
                    Some(schema),
                    StorageModeChoice::Persistent,
                )
                .map_err(|e| CassandraReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                })
            }
            Err(e) => Err(CassandraReply::Error {
                code: "server_error".into(),
                message: format!("{e:?}"),
            }),
        }
    }
}

fn refuse(verb: &str, err: CompatError) -> CassandraReply {
    record_must_not(ProtocolId::Cassandra.as_str(), verb);
    CassandraReply::Error {
        code: err.code().into(),
        message: err.to_string(),
    }
}

/// Dispatch a normalized CQL verb (`SELECT`, `INSERT`, …) before IR.
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
            // CREATE KEYSPACE ks / CREATE TABLE t (...)
            let kind = args.first().map(|s| s.to_ascii_uppercase()).unwrap_or_default();
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
                Ok(_) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        "DROP" => {
            let table = args
                .iter()
                .find(|a| a.to_ascii_uppercase() != "TABLE")
                .copied()
                .unwrap_or("t");
            let mut cat = session.catalog.write().expect("catalog");
            match cat.drop_container(&session.namespace, table.trim_matches('"')) {
                Ok(()) | Err(TypeError::NotFound) => CassandraReply::Ok,
                Err(e) => CassandraReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "ALTER" => CassandraReply::Ok,
        "PREPARE" => {
            let id = format!("prep-{}", args.first().unwrap_or(&"q"));
            CassandraReply::Prepared(id)
        }
        "EXECUTE" | "QUERY" => {
            // EXECUTE/QUERY with synthetic args: table key value
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            let id = match session.ensure_table(table) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            match cat.get(id, key) {
                Ok(Some(v)) => CassandraReply::Rows(vec![(
                    key.into(),
                    String::from_utf8_lossy(v).into_owned(),
                )]),
                Ok(None) => CassandraReply::Rows(vec![]),
                Err(e) => CassandraReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "INSERT" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            let val = args.get(2).copied().unwrap_or("");
            let id = match session.ensure_table(table) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            match cat.put(id, key, val.as_bytes().to_vec()) {
                Ok(()) => CassandraReply::Ok,
                Err(e) => CassandraReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "SELECT" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied();
            let id = match session.ensure_table(table) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            if let Some(k) = key {
                match cat.get(id, k) {
                    Ok(Some(v)) => CassandraReply::Rows(vec![(
                        k.into(),
                        String::from_utf8_lossy(v).into_owned(),
                    )]),
                    Ok(None) => CassandraReply::Rows(vec![]),
                    Err(e) => CassandraReply::Error {
                        code: "server_error".into(),
                        message: format!("{e:?}"),
                    },
                }
            } else {
                match cat.keys(id) {
                    Ok(keys) => {
                        let mut rows = Vec::new();
                        for k in keys {
                            if let Ok(Some(v)) = cat.get(id, &k) {
                                rows.push((k, String::from_utf8_lossy(v).into_owned()));
                            }
                        }
                        CassandraReply::Rows(rows)
                    }
                    Err(e) => CassandraReply::Error {
                        code: "server_error".into(),
                        message: format!("{e:?}"),
                    },
                }
            }
        }
        "UPDATE" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            let val = args.get(2).copied().unwrap_or("");
            let id = match session.ensure_table(table) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            match cat.put(id, key, val.as_bytes().to_vec()) {
                Ok(()) => CassandraReply::Ok,
                Err(e) => CassandraReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "DELETE" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            let id = match session.ensure_table(table) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            match cat.delete_row(id, key) {
                Ok(_) => CassandraReply::Ok,
                Err(e) => CassandraReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
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
