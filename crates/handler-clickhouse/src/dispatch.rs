//! Classify-gated ClickHouse SQL verbs (shared by native + HTTP entrypoints).

use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_types::{
    ContainerCatalog, ContainerId, ContainerSchema, Field, L3Model, StorageModeChoice, TypeError,
    ValueDomain,
};

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

    fn ensure_table(&self, table: &str) -> Result<ContainerId, ClickHouseReply> {
        let mut cat = self.catalog.write().expect("catalog");
        match cat.describe(&self.namespace, table) {
            Ok(c) => {
                if c.model != L3Model::RelationalTable {
                    return Err(ClickHouseReply::Error {
                        code: "compat_must_not".into(),
                        message: "wrong type".into(),
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
                .map_err(|e| ClickHouseReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                })
            }
            Err(e) => Err(ClickHouseReply::Error {
                code: "server_error".into(),
                message: format!("{e:?}"),
            }),
        }
    }
}

fn refuse(protocol: ProtocolId, verb: &str, err: CompatError) -> ClickHouseReply {
    record_must_not(protocol.as_str(), verb);
    ClickHouseReply::Error {
        code: err.code().into(),
        message: err.to_string(),
    }
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
                .find(|a| !matches!(a.to_ascii_uppercase().as_str(), "TABLE" | "IF" | "NOT" | "EXISTS"))
                .copied()
                .unwrap_or("t");
            match session.ensure_table(table) {
                Ok(_) => ClickHouseReply::Ok,
                Err(e) => e,
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
                Ok(()) => ClickHouseReply::Ok,
                Err(e) => ClickHouseReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "SELECT" | "SELECT_WHERE" => {
            let table = args.first().copied().unwrap_or("t");
            let filter = args.get(1).copied();
            let id = match session.ensure_table(table) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            match cat.keys(id) {
                Ok(keys) => {
                    let mut rows = Vec::new();
                    for k in keys {
                        if let Some(f) = filter {
                            if k != f && !k.contains(f) {
                                continue;
                            }
                        }
                        if let Ok(Some(v)) = cat.get(id, &k) {
                            rows.push((k, String::from_utf8_lossy(v).into_owned()));
                        }
                    }
                    ClickHouseReply::Rows(rows)
                }
                Err(e) => ClickHouseReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
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
