//! Classify-gated WebDAV methods.

use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_types::{ContainerCatalog, ContainerId, L3Model, StorageModeChoice, TypeError};

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
}

impl SessionState {
    pub fn demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            collection: "dav".into(),
            profile: DialectProfile::HandlersComplete,
        }
    }

    fn ensure_collection(&self) -> Result<ContainerId, WebDavReply> {
        let mut cat = self.catalog.write().expect("catalog");
        match cat.describe(&self.namespace, &self.collection) {
            Ok(c) => {
                if c.model != L3Model::KvStore {
                    return Err(WebDavReply::Error {
                        code: "compat_must_not".into(),
                        message: "wrong type".into(),
                    });
                }
                Ok(c.id)
            }
            Err(TypeError::NotFound) => cat
                .create(
                    &self.namespace,
                    &self.collection,
                    L3Model::KvStore,
                    false,
                    None,
                    StorageModeChoice::Persistent,
                )
                .map_err(|e| WebDavReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                }),
            Err(e) => Err(WebDavReply::Error {
                code: "server_error".into(),
                message: format!("{e:?}"),
            }),
        }
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
        "OPTIONS" | "HEAD" => WebDavReply::Ok,
        "MKCOL" => {
            let path = args.first().copied().unwrap_or("/");
            // Nested collection marker as empty key with trailing slash convention.
            let key = format!("{}/", path_key(path));
            let id = match session.ensure_collection() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            match cat.put(id, &key, Vec::new()) {
                Ok(()) => WebDavReply::Ok,
                Err(e) => WebDavReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "PUT" => {
            let path = args.first().copied().unwrap_or("/f");
            let body = args.get(1).copied().unwrap_or("").as_bytes().to_vec();
            let id = match session.ensure_collection() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            match cat.put(id, &path_key(path), body) {
                Ok(()) => WebDavReply::Ok,
                Err(e) => WebDavReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "GET" => {
            let path = args.first().copied().unwrap_or("/f");
            let id = match session.ensure_collection() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            match cat.get(id, &path_key(path)) {
                Ok(Some(v)) => WebDavReply::Body(v.to_vec()),
                Ok(None) => WebDavReply::Error {
                    code: "not_found".into(),
                    message: path.into(),
                },
                Err(e) => WebDavReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "DELETE" => {
            let path = args.first().copied().unwrap_or("/f");
            let id = match session.ensure_collection() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            let _ = cat.delete_row(id, &path_key(path));
            WebDavReply::Ok
        }
        "PROPFIND" => {
            let id = match session.ensure_collection() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            let keys = match cat.keys(id) {
                Ok(k) => k,
                Err(e) => {
                    return WebDavReply::Error {
                        code: "server_error".into(),
                        message: format!("{e:?}"),
                    }
                }
            };
            let mut xml = String::from("<?xml version=\"1.0\"?><D:multistatus xmlns:D=\"DAV:\">");
            for k in keys {
                xml.push_str(&format!(
                    "<D:response><D:href>/{k}</D:href><D:propstat><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>"
                ));
            }
            xml.push_str("</D:multistatus>");
            WebDavReply::MultiStatus(xml)
        }
        "MOVE" | "COPY" => {
            let src = args.first().copied().unwrap_or("/a");
            let dst = args.get(1).copied().unwrap_or("/b");
            let id = match session.ensure_collection() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            let data = match cat.get(id, &path_key(src)) {
                Ok(Some(v)) => v.to_vec(),
                Ok(None) => {
                    return WebDavReply::Error {
                        code: "not_found".into(),
                        message: src.into(),
                    }
                }
                Err(e) => {
                    return WebDavReply::Error {
                        code: "server_error".into(),
                        message: format!("{e:?}"),
                    }
                }
            };
            if let Err(e) = cat.put(id, &path_key(dst), data) {
                return WebDavReply::Error {
                    code: "server_error".into(),
                    message: format!("{e:?}"),
                };
            }
            if upper == "MOVE" {
                let _ = cat.delete_row(id, &path_key(src));
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
    fn lock_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        match dispatch(&mut s, "LOCK", &["/f"]) {
            WebDavReply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
            other => panic!("{other:?}"),
        }
    }
}
