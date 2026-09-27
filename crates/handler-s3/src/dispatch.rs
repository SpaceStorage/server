//! Classify-gated S3 API verbs via shared `PlannerEngine` (005 T086).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine, QueryOptions, QueryResult};
use spacestorage_types::{ContainerCatalog, L3Model};

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

    fn engine(&self) -> PlannerEngine {
        PlannerEngine::new(Arc::clone(&self.catalog))
    }

    fn exec(&self, req: LogicalRequest) -> Result<QueryResult, S3Reply> {
        self.engine()
            .execute_blocking(
                &self.namespace,
                "s3",
                "s3",
                req,
                QueryOptions::default(),
                CancelToken::new(),
            )
            .map_err(|e| S3Reply::Error {
                code: "InternalError".into(),
                message: e.to_string(),
            })
    }

    fn ensure_bucket(&self, bucket: &str) -> Result<(), S3Reply> {
        self.exec(LogicalRequest::TypeOp {
            container: bucket.into(),
            op: "ensure_kv".into(),
        })?;
        Ok(())
    }
}

fn refuse(verb: &str, err: CompatError) -> S3Reply {
    record_must_not(ProtocolId::S3.as_str(), verb);
    S3Reply::Error {
        code: err.code().into(),
        message: err.to_string(),
    }
}

pub fn put_object_bytes(
    session: &mut SessionState,
    bucket: &str,
    key: &str,
    body: Vec<u8>,
) -> S3Reply {
    if let Err(e) = session.ensure_bucket(bucket) {
        return e;
    }
    match session.exec(LogicalRequest::Object {
        container: bucket.into(),
        key: key.into(),
        bytes: body,
    }) {
        Ok(_) => S3Reply::Ok,
        Err(e) => e,
    }
}

pub fn upload_part_bytes(session: &mut SessionState, upload_id: &str, body: Vec<u8>) -> S3Reply {
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
                    xml.push_str(&format!(
                        "<Bucket><Name>{}</Name></Bucket>",
                        c.name.as_str()
                    ));
                }
            }
            xml.push_str("</Buckets></ListAllMyBucketsResult>");
            S3Reply::Xml(xml)
        }
        "CREATEBUCKET" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            match session.ensure_bucket(bucket) {
                Ok(()) => S3Reply::Ok,
                Err(e) => e,
            }
        }
        "PUTOBJECT" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let key = args.get(1).copied().unwrap_or("key");
            let body = args.get(2).copied().unwrap_or("").as_bytes().to_vec();
            put_object_bytes(session, bucket, key, body)
        }
        "GETOBJECT" | "HEADOBJECT" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let key = args.get(1).copied().unwrap_or("key");
            if let Err(e) = session.ensure_bucket(bucket) {
                return e;
            }
            match session.exec(LogicalRequest::Point {
                container: bucket.into(),
                key: key.into(),
            }) {
                Ok(r) if r.rows.is_empty() => S3Reply::Error {
                    code: "NoSuchKey".into(),
                    message: key.into(),
                },
                Ok(r) => {
                    let v = r.rows[0]
                        .get(1)
                        .and_then(|x| x.as_ref())
                        .map(|s| s.as_bytes().to_vec())
                        .unwrap_or_default();
                    if upper == "HEADOBJECT" {
                        S3Reply::Ok
                    } else {
                        S3Reply::Body(v)
                    }
                }
                Err(e) => e,
            }
        }
        "DELETEOBJECT" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let key = args.get(1).copied().unwrap_or("key");
            if let Err(e) = session.ensure_bucket(bucket) {
                return e;
            }
            let _ = session.exec(LogicalRequest::Mutate {
                container: bucket.into(),
                op: "delete".into(),
                sql: key.into(),
            });
            S3Reply::Ok
        }
        "LISTOBJECTSV2" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let prefix = args.get(1).copied().unwrap_or("");
            if let Err(e) = session.ensure_bucket(bucket) {
                return e;
            }
            match session.exec(LogicalRequest::Scan {
                container: bucket.into(),
                filter: if prefix.is_empty() {
                    None
                } else {
                    Some(prefix.into())
                },
            }) {
                Ok(r) => {
                    let mut xml = String::from("<ListBucketResult><Contents>");
                    for row in r.rows {
                        let k = row.first().and_then(|x| x.clone()).unwrap_or_default();
                        if prefix.is_empty() || k.starts_with(prefix) {
                            xml.push_str(&format!("<Key>{k}</Key>"));
                        }
                    }
                    xml.push_str("</Contents></ListBucketResult>");
                    S3Reply::Xml(xml)
                }
                Err(e) => e,
            }
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
            upload_part_bytes(session, upload_id, body)
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
            put_object_bytes(session, &bucket, &key, assembled)
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
