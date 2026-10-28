//! Classify-gated WebDAV methods via shared `PlannerEngine` (005 T086).

use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, LimitServices,
    PrincipalHold, ProtocolId,
};
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine, QueryOptions, QueryResult};
use spacestorage_types::ContainerCatalog;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebDavReply {
    Ok,
    MultiStatus(String),
    Body(Vec<u8>),
    Error { code: String, message: String },
}

pub struct SessionState {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub namespace: String,
    pub collection: String,
    pub profile: DialectProfile,
    pub limit_services: LimitServices,
    pub principal_hold: Option<PrincipalHold>,
    pub tenancy: Option<Arc<spacestorage_tenancy::Tenancy>>,
    pub connection_quota: Option<spacestorage_tenancy::ConnectionQuotaHold>,
    pub principal: Option<String>,
}

impl SessionState {
    pub fn demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            collection: "dav".into(),
            profile: DialectProfile::HandlersComplete,
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: None,
            connection_quota: None,
            principal: None,
        }
    }

    fn engine(&self) -> PlannerEngine {
        let mut engine = PlannerEngine::new(Arc::clone(&self.catalog));
        if let Some(t) = &self.tenancy {
            engine = engine.with_tenancy(Arc::clone(t));
        }
        engine
    }

    fn exec_opts(&self) -> QueryOptions {
        let mut options = QueryOptions::default();
        if let Some(login) = self.principal.as_deref() {
            options.principal = Some(login.to_string());
        }
        options
    }

    fn exec(&self, req: LogicalRequest) -> Result<QueryResult, WebDavReply> {
        self.engine()
            .execute_blocking(
                &self.namespace,
                "dav",
                "webdav",
                req,
                self.exec_opts(),
                CancelToken::new(),
            )
            .map_err(|e| WebDavReply::Error {
                code: "server_error".into(),
                message: e.to_string(),
            })
    }

    fn ensure_collection(&self) -> Result<(), WebDavReply> {
        self.exec(LogicalRequest::TypeOp {
            container: self.collection.clone(),
            op: "ensure_kv".into(),
        })?;
        Ok(())
    }
}

fn refuse(verb: &str, err: CompatError) -> WebDavReply {
    record_must_not(ProtocolId::WebDav.as_str(), verb);
    WebDavReply::Error {
        code: err.code().into(),
        message: err.to_string(),
    }
}

fn path_key(path: &str) -> String {
    path.trim_start_matches('/').to_string()
}

pub fn dispatch(session: &mut SessionState, verb: &str, args: &[&str]) -> WebDavReply {
    let upper = verb.to_ascii_uppercase().replace('-', "_");
    match classify_outcome(session.profile, ProtocolId::WebDav, &upper) {
        ClassifyOutcome::Must => {}
        ClassifyOutcome::MustNot(err) => return refuse(&upper, err),
    }

    match upper.as_str() {
        "MKCOL" => {
            let path = args.first().copied().unwrap_or("/");
            let key = format!("{}/", path_key(path));
            if let Err(e) = session.ensure_collection() {
                return e;
            }
            match session.exec(LogicalRequest::Object {
                container: session.collection.clone(),
                key,
                bytes: Vec::new(),
            }) {
                Ok(_) => WebDavReply::Ok,
                Err(e) => e,
            }
        }
        "PUT" => {
            let path = args.first().copied().unwrap_or("/f");
            let body = args.get(1).copied().unwrap_or("").as_bytes().to_vec();
            let key = path_key(path);
            let limits = session.limit_services.limits();
            if let Err(e) = limits.check_key(key.len() as u64) {
                return WebDavReply::Error {
                    code: "limit_exceeded".into(),
                    message: e.to_string(),
                };
            }
            if let Err(e) = limits.check_value(body.len() as u64) {
                return WebDavReply::Error {
                    code: "limit_exceeded".into(),
                    message: e.to_string(),
                };
            }
            if let Err(e) = session.ensure_collection() {
                return e;
            }
            match session.exec(LogicalRequest::Object {
                container: session.collection.clone(),
                key,
                bytes: body,
            }) {
                Ok(_) => WebDavReply::Ok,
                Err(e) => e,
            }
        }
        "GET" => {
            let path = args.first().copied().unwrap_or("/f");
            if let Err(e) = session.ensure_collection() {
                return e;
            }
            match session.exec(LogicalRequest::Point {
                container: session.collection.clone(),
                key: path_key(path),
            }) {
                Ok(r) if r.rows.is_empty() => WebDavReply::Error {
                    code: "not_found".into(),
                    message: path.into(),
                },
                Ok(r) => {
                    let v = r.rows[0]
                        .get(1)
                        .and_then(|x| x.as_ref())
                        .map(|s| s.as_bytes().to_vec())
                        .unwrap_or_default();
                    WebDavReply::Body(v)
                }
                Err(e) => e,
            }
        }
        "DELETE" => {
            let path = args.first().copied().unwrap_or("/f");
            if let Err(e) = session.ensure_collection() {
                return e;
            }
            let _ = session.exec(LogicalRequest::Mutate {
                container: session.collection.clone(),
                op: "delete".into(),
                sql: path_key(path),
            });
            WebDavReply::Ok
        }
        "PROPFIND" => {
            if let Err(e) = session.ensure_collection() {
                return e;
            }
            match session.exec(LogicalRequest::Scan {
                container: session.collection.clone(),
                filter: None,
            }) {
                Ok(r) => {
                    let mut xml =
                        String::from("<?xml version=\"1.0\"?><D:multistatus xmlns:D=\"DAV:\">");
                    for row in r.rows {
                        let k = row.first().and_then(|x| x.clone()).unwrap_or_default();
                        xml.push_str(&format!(
                            "<D:response><D:href>/{k}</D:href><D:propstat><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>"
                        ));
                    }
                    xml.push_str("</D:multistatus>");
                    WebDavReply::MultiStatus(xml)
                }
                Err(e) => e,
            }
        }
        "MOVE" | "COPY" => {
            let src = args.first().copied().unwrap_or("/a");
            let dst = args.get(1).copied().unwrap_or("/b");
            if let Err(e) = session.ensure_collection() {
                return e;
            }
            let data = match session.exec(LogicalRequest::Point {
                container: session.collection.clone(),
                key: path_key(src),
            }) {
                Ok(r) if r.rows.is_empty() => {
                    return WebDavReply::Error {
                        code: "not_found".into(),
                        message: src.into(),
                    }
                }
                Ok(r) => r.rows[0]
                    .get(1)
                    .and_then(|x| x.as_ref())
                    .map(|s| s.as_bytes().to_vec())
                    .unwrap_or_default(),
                Err(e) => return e,
            };
            if let Err(e) = session.exec(LogicalRequest::Object {
                container: session.collection.clone(),
                key: path_key(dst),
                bytes: data,
            }) {
                return e;
            }
            if upper == "MOVE" {
                let _ = session.exec(LogicalRequest::Mutate {
                    container: session.collection.clone(),
                    op: "delete".into(),
                    sql: path_key(src),
                });
            }
            WebDavReply::Ok
        }
        other => refuse(
            other,
            CompatError::CompatMustNot {
                protocol: ProtocolId::WebDav.as_str().into(),
                verb: other.into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn must_verbs_smoke() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        assert!(matches!(dispatch(&mut s, "MKCOL", &["/dir"]), WebDavReply::Ok));
        assert!(matches!(
            dispatch(&mut s, "PUT", &["/dir/f", "hi"]),
            WebDavReply::Ok
        ));
        match dispatch(&mut s, "GET", &["/dir/f"]) {
            WebDavReply::Body(b) => assert_eq!(b, b"hi"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            dispatch(&mut s, "COPY", &["/dir/f", "/dir/g"]),
            WebDavReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "MOVE", &["/dir/g", "/dir/h"]),
            WebDavReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "PROPFIND", &["/"]),
            WebDavReply::MultiStatus(_)
        ));
        assert!(matches!(
            dispatch(&mut s, "DELETE", &["/dir/h"]),
            WebDavReply::Ok
        ));
    }

    #[test]
    fn head_and_options_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        for verb in ["HEAD", "OPTIONS"] {
            match dispatch(&mut s, verb, &["/f"]) {
                WebDavReply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
                other => panic!("{verb}: {other:?}"),
            }
        }
    }

    #[test]
    fn lock_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        match dispatch(&mut s, "LOCK", &["/f"]) {
            WebDavReply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
            other => panic!("{other:?}"),
        }
    }
}
