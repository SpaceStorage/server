//! Classify-gated S3 API verbs.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_types::{ContainerCatalog, ContainerId, L3Model, StorageModeChoice, TypeError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3Reply {
    Ok,
    Xml(String),
    Body(Vec<u8>),
    Error { code: String, message: String },
}

pub struct SessionState {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub namespace: String,
    pub profile: DialectProfile,
    /// multipart upload id → (bucket, key, parts)
    pub multipart: HashMap<String, (String, String, Vec<Vec<u8>>)>,
}

impl SessionState {
    pub fn demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            multipart: HashMap::new(),
        }
    }

    fn ensure_bucket(&self, bucket: &str) -> Result<ContainerId, S3Reply> {
        let mut cat = self.catalog.write().expect("catalog");
        match cat.describe(&self.namespace, bucket) {
            Ok(c) => {
                if c.model != L3Model::KvStore {
                    return Err(S3Reply::Error {
                        code: "InvalidBucketState".into(),
                        message: "wrong type".into(),
                    });
                }
                Ok(c.id)
            }
            Err(TypeError::NotFound) => cat
                .create(
                    &self.namespace,
                    bucket,
                    L3Model::KvStore,
                    false,
                    None,
                    StorageModeChoice::Persistent,
                )
                .map_err(|e| S3Reply::Error {
                    code: "InternalError".into(),
                    message: format!("{e:?}"),
                }),
            Err(e) => Err(S3Reply::Error {
                code: "InternalError".into(),
                message: format!("{e:?}"),
            }),
        }
    }
}

fn refuse(verb: &str, err: CompatError) -> S3Reply {
    record_must_not(ProtocolId::S3.as_str(), verb);
    S3Reply::Error {
        code: err.code().into(),
        message: err.to_string(),
    }
}

pub fn dispatch(session: &mut SessionState, verb: &str, args: &[&str]) -> S3Reply {
    let upper = verb.to_ascii_uppercase().replace('-', "_");
    match classify_outcome(session.profile, ProtocolId::S3, &upper) {
        ClassifyOutcome::Must => {}
        ClassifyOutcome::MustNot(err) => return refuse(&upper, err),
    }

    match upper.as_str() {
        "LISTBUCKETS" => {
            let cat = session.catalog.read().expect("catalog");
            let mut xml = String::from("<ListAllMyBucketsResult><Buckets>");
            for c in cat.list() {
                if c.namespace.as_str() == session.namespace && c.model == L3Model::KvStore {
                    xml.push_str(&format!("<Bucket><Name>{}</Name></Bucket>", c.name.as_str()));
                }
            }
            xml.push_str("</Buckets></ListAllMyBucketsResult>");
            S3Reply::Xml(xml)
        }
        "CREATEBUCKET" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            match session.ensure_bucket(bucket) {
                Ok(_) => S3Reply::Ok,
                Err(e) => e,
            }
        }
        "PUTOBJECT" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let key = args.get(1).copied().unwrap_or("key");
            let body = args.get(2).copied().unwrap_or("").as_bytes().to_vec();
            let id = match session.ensure_bucket(bucket) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            match cat.put(id, key, body) {
                Ok(()) => S3Reply::Ok,
                Err(e) => S3Reply::Error {
                    code: "InternalError".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "GETOBJECT" | "HEADOBJECT" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let key = args.get(1).copied().unwrap_or("key");
            let id = match session.ensure_bucket(bucket) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            match cat.get(id, key) {
                Ok(Some(v)) => {
                    if upper == "HEADOBJECT" {
                        S3Reply::Ok
                    } else {
                        S3Reply::Body(v.to_vec())
                    }
                }
                Ok(None) => S3Reply::Error {
                    code: "NoSuchKey".into(),
                    message: key.into(),
                },
                Err(e) => S3Reply::Error {
                    code: "InternalError".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        "DELETEOBJECT" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let key = args.get(1).copied().unwrap_or("key");
            let id = match session.ensure_bucket(bucket) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            let _ = cat.delete_row(id, key);
            S3Reply::Ok
        }
        "LISTOBJECTSV2" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let prefix = args.get(1).copied().unwrap_or("");
            let id = match session.ensure_bucket(bucket) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            let keys = match cat.keys(id) {
                Ok(k) => k,
                Err(e) => {
                    return S3Reply::Error {
                        code: "InternalError".into(),
                        message: format!("{e:?}"),
                    }
                }
            };
            let mut xml = String::from("<ListBucketResult><Contents>");
            for k in keys {
                if prefix.is_empty() || k.starts_with(prefix) {
                    xml.push_str(&format!("<Key>{k}</Key>"));
                }
            }
            xml.push_str("</Contents></ListBucketResult>");
            S3Reply::Xml(xml)
        }
        "CREATEMULTIPARTUPLOAD" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let key = args.get(1).copied().unwrap_or("key");
            let _ = session.ensure_bucket(bucket);
            let upload_id = format!("mpu-{}", session.multipart.len() + 1);
            session
                .multipart
                .insert(upload_id.clone(), (bucket.into(), key.into(), Vec::new()));
            S3Reply::Xml(format!(
                "<InitiateMultipartUploadResult><UploadId>{upload_id}</UploadId></InitiateMultipartUploadResult>"
            ))
        }
        "UPLOADPART" => {
            let upload_id = args.first().copied().unwrap_or("");
            let body = args.get(1).copied().unwrap_or("").as_bytes().to_vec();
            match session.multipart.get_mut(upload_id) {
                Some((_, _, parts)) => {
                    parts.push(body);
                    S3Reply::Ok
                }
                None => S3Reply::Error {
                    code: "NoSuchUpload".into(),
                    message: upload_id.into(),
                },
            }
        }
        "COMPLETEMULTIPARTUPLOAD" => {
            let upload_id = args.first().copied().unwrap_or("");
            let Some((bucket, key, parts)) = session.multipart.remove(upload_id) else {
                return S3Reply::Error {
                    code: "NoSuchUpload".into(),
                    message: upload_id.into(),
                };
            };
            let mut assembled = Vec::new();
            for p in parts {
                assembled.extend_from_slice(&p);
            }
            let id = match session.ensure_bucket(&bucket) {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            match cat.put(id, &key, assembled) {
                Ok(()) => S3Reply::Ok,
                Err(e) => S3Reply::Error {
                    code: "InternalError".into(),
                    message: format!("{e:?}"),
                },
            }
        }
        other => refuse(
            other,
            CompatError::CompatMustNot {
                protocol: ProtocolId::S3.as_str().into(),
                verb: other.into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_crud_and_multipart() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATEBUCKET", &["b"]),
            S3Reply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "PUTOBJECT", &["b", "k", "v"]),
            S3Reply::Ok
        ));
        match dispatch(&mut s, "GETOBJECT", &["b", "k"]) {
            S3Reply::Body(b) => assert_eq!(b, b"v"),
            other => panic!("{other:?}"),
        }
        match dispatch(&mut s, "CREATEMULTIPARTUPLOAD", &["b", "m"]) {
            S3Reply::Xml(x) => assert!(x.contains("UploadId")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn versioning_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        match dispatch(&mut s, "VERSIONING", &["b"]) {
            S3Reply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
            other => panic!("{other:?}"),
        }
    }
}
