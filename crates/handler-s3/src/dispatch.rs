//! Classify-gated S3 API verbs via shared `PlannerEngine` (005 T086).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use spacestorage_authz::Verb;
use spacestorage_compat::{
    ClassifyOutcome, CompatError, DialectProfile, LimitServices, PrincipalHold, ProtocolId,
    classify_outcome, record_must_not,
};
use spacestorage_query::options::Sourced;
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine, QueryOptions, QueryResult};
use spacestorage_types::ContainerCatalog;

use crate::etag::{multipart_etag, single_part_etag};
use crate::meta::{self, ObjectMeta};
use crate::multipart::MultipartRegistry;
use crate::xml::{
    bucket_location_xml, complete_multipart_upload_result, delete_objects_result_xml,
    initiate_multipart_xml, list_buckets_xml, list_multipart_uploads_xml, list_objects_v2_xml,
    list_parts_xml, parse_complete_multipart_parts, parse_delete_keys, s3_bucket_list_name,
    s3_bucket_name_valid,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectReply {
    pub bytes: Vec<u8>,
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3Reply {
    Ok,
    /// DELETE / AbortMultipartUpload — AWS returns 204.
    NoContent,
    OkHeaders(Vec<(String, String)>),
    Xml(String),
    Body(ObjectReply),
    Head {
        content_length: u64,
        headers: Vec<(String, String)>,
    },
    /// Logical object stored as fixed-window chunks. The handler writes one window at a time.
    ChunkedObject {
        bucket: String,
        key: String,
        staging: String,
        total: u64,
        chunk_count: u32,
        window: usize,
        headers: Vec<(String, String)>,
        status: u16,
        range: Option<(usize, usize)>,
    },
    Error {
        code: String,
        message: String,
    },
}

pub struct SessionState {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub namespace: String,
    pub profile: DialectProfile,
    pub multipart: Arc<RwLock<MultipartRegistry>>,
    pub limit_services: LimitServices,
    pub principal_hold: Option<PrincipalHold>,
    pub tenancy: Option<Arc<spacestorage_tenancy::Tenancy>>,
    pub connection_quota: Option<spacestorage_tenancy::ConnectionQuotaHold>,
    pub principal: Option<String>,
    pub region: String,
    /// `ensure_<model>` passed to the type op. Default `ensure_object_collection` (T022).
    pub bucket_type_op: String,
    pub query_options: QueryOptions,
    /// When false, refuse data ops (FR-006 unbound principal).
    pub namespace_bound: bool,
    pub multipart_max_parts: u32,
    pub multipart_min_part: u64,
    /// Closed-vocabulary bits from the 014 access key (`Verb`).
    pub grants: u16,
    /// `x-spacestorage-type` for the next CreateBucket, if the client set one.
    pub requested_type: Option<String>,
    /// `x-spacestorage-opt-*` pairs for the next CreateBucket.
    pub requested_opts: Vec<(String, String)>,
}

impl SessionState {
    pub fn demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            multipart: Arc::new(RwLock::new(MultipartRegistry::default())),
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: None,
            connection_quota: None,
            principal: None,
            region: "us-east-1".into(),
            bucket_type_op: "ensure_object_collection".into(),
            query_options: QueryOptions::default(),
            namespace_bound: true,
            multipart_max_parts: 10_000,
            multipart_min_part: 5 * 1024 * 1024,
            grants: Verb::IMPLICIT_TENANT.bits(),
            requested_type: None,
            requested_opts: Vec::new(),
        }
    }

    pub fn with_multipart_limits(mut self, min_part: u64, max_parts: u32) -> Self {
        self.multipart_min_part = min_part;
        self.multipart_max_parts = max_parts;
        self.multipart = Arc::new(RwLock::new(MultipartRegistry::new(
            std::time::Duration::from_secs(24 * 3600),
            min_part,
            max_parts,
        )));
        self
    }

    fn engine(&self) -> PlannerEngine {
        let mut engine = PlannerEngine::new(Arc::clone(&self.catalog));
        if let Some(t) = &self.tenancy {
            engine = engine.with_tenancy(Arc::clone(t));
        }
        engine
    }

    fn exec_opts(&self) -> QueryOptions {
        let mut options = self.query_options.clone();
        if let Some(login) = self.principal.as_deref() {
            options.principal = Some(login.to_string());
        }
        options
    }

    /// Catalog / durable path — never block Tokio workers when a runtime is present (T023a).
    fn exec(&self, req: LogicalRequest) -> Result<QueryResult, S3Reply> {
        let engine = self.engine();
        let namespace = self.namespace.clone();
        let opts = self.exec_opts();
        let cancel = CancelToken::new();

        let run = move || engine.execute_blocking(&namespace, "s3", "s3", req, opts, cancel);

        let outcome: Result<QueryResult, spacestorage_query::ExecError> =
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let multithreaded = matches!(
                    handle.runtime_flavor(),
                    tokio::runtime::RuntimeFlavor::MultiThread
                );
                if multithreaded {
                    // Durable `execute_blocking` runs on the spawn_blocking pool (T075 / R8).
                    // Bare `Handle::block_on(spawn_blocking)` deadlocks Suite A
                    // (`worker_threads = 2`): every worker sits in `block_on` and never
                    // polls the JoinHandle. `block_in_place` moves this worker aside so
                    // `block_on` can drive that JoinHandle on the blocking thread.
                    tokio::task::block_in_place(|| {
                        handle.block_on(async move {
                            tokio::task::spawn_blocking(run)
                                .await
                                .unwrap_or_else(|join| {
                                    Err(spacestorage_query::ExecError::NotSupported(
                                        join.to_string(),
                                    ))
                                })
                        })
                    })
                } else {
                    run()
                }
            } else {
                run()
            };

        outcome.map_err(|e| {
            let msg = e.to_string();
            if msg.contains("quorum") || msg.contains("Quorum") {
                S3Reply::Error {
                    code: "QuorumUnsatisfiable".into(),
                    message: msg,
                }
            } else if msg.contains("unknown_type") {
                S3Reply::Error {
                    code: "InvalidArgument".into(),
                    message: msg,
                }
            } else {
                S3Reply::Error {
                    code: "InternalError".into(),
                    message: msg,
                }
            }
        })
    }

    fn ensure_bucket(&self, bucket: &str, type_op: Option<&str>) -> Result<(), S3Reply> {
        let op = type_op.unwrap_or(self.bucket_type_op.as_str());
        self.exec(LogicalRequest::TypeOp {
            container: bucket.into(),
            op: op.into(),
        })?;
        Ok(())
    }

    fn bucket_exists(&self, bucket: &str) -> bool {
        self.catalog
            .read()
            .expect("catalog")
            .describe(&self.namespace, bucket)
            .is_ok()
    }

    fn bucket_empty(&self, bucket: &str) -> Result<bool, S3Reply> {
        let keys = self.user_keys(bucket)?;
        let uploads = merged_multipart_uploads(self, bucket)?;
        Ok(keys.is_empty() && uploads.is_empty())
    }

    /// `Ok(false)` only when the point read succeeded and the key is absent.
    /// A quorum miss or `exec` error fails closed so `If-None-Match: *` cannot
    /// overwrite a key the session could not confirm (FR-009).
    pub(crate) fn object_key_exists(&self, bucket: &str, key: &str) -> Result<bool, S3Reply> {
        if !self.bucket_exists(bucket) {
            return Ok(false);
        }
        require_read_quorum(self)?;
        match self.exec(LogicalRequest::Point {
            container: bucket.into(),
            key: key.into(),
        }) {
            Ok(r) => Ok(!r.rows.is_empty()),
            Err(e) => Err(e),
        }
    }

    /// Key listing from the container row map. Avoids `LogicalRequest::Scan`, which
    /// blocked the live TCP harness under the default write quorum.
    /// A session read quorum this process cannot meet fails closed (FR-009).
    fn user_keys(&self, bucket: &str) -> Result<Vec<String>, S3Reply> {
        if !self.bucket_exists(bucket) {
            return Err(S3Reply::Error {
                code: "NoSuchBucket".into(),
                message: bucket.into(),
            });
        }
        require_read_quorum(self)?;
        let cat = self.catalog.read().expect("catalog");
        let described = cat
            .describe(&self.namespace, bucket)
            .map_err(|e| S3Reply::Error {
                code: "NoSuchBucket".into(),
                message: e.to_string(),
            })?;
        let id = described.id;
        let keys = cat.keys(id).map_err(|e| S3Reply::Error {
            code: "InternalError".into(),
            message: e.to_string(),
        })?;
        Ok(keys.into_iter().filter(|k| !k.starts_with('\0')).collect())
    }

    fn require_grant(&self, verb: Verb) -> Result<(), S3Reply> {
        if Verb::from_bits(self.grants).contains(verb) {
            Ok(())
        } else {
            Err(S3Reply::Error {
                code: "AccessDenied".into(),
                message: "insufficient grants".into(),
            })
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

/// HTTP router MustNot short-circuit (before IR) for verbs outside the classified MUST set.
pub fn compat_must_not_reply(verb: &str) -> S3Reply {
    refuse(
        verb,
        CompatError::CompatMustNot {
            protocol: ProtocolId::S3.as_str().into(),
            verb: verb.into(),
        },
    )
}

/// Thread `X-SpaceStorage-Quorum` / `X-SpaceStorage-Timeout` into session options (FR-009).
pub fn apply_query_headers(
    session: &mut SessionState,
    quorum: Option<&str>,
    timeout: Option<&str>,
) {
    if let Some(q) = quorum {
        session.query_options.quorum_write = Sourced::Session(q.to_string());
        session.query_options.quorum_read = Sourced::Session(q.to_string());
    }
    if let Some(t) = timeout {
        if let Ok(secs) = t.parse::<u64>() {
            session.query_options.timeout = Sourced::Session(std::time::Duration::from_secs(secs));
        }
    }
}

pub fn put_object_bytes(
    session: &mut SessionState,
    bucket: &str,
    key: &str,
    body: Vec<u8>,
) -> S3Reply {
    put_object_with_meta(session, bucket, key, body, ObjectMeta::default())
}

pub fn put_object_with_meta(
    session: &mut SessionState,
    bucket: &str,
    key: &str,
    body: Vec<u8>,
    mut meta: ObjectMeta,
) -> S3Reply {
    if let Err(e) = session.require_grant(Verb::WRITE) {
        return e;
    }
    if !session.namespace_bound {
        return S3Reply::Error {
            code: "AccessDenied".into(),
            message: "UnboundCredential".into(),
        };
    }
    let limits = session.limit_services.limits();
    if let Err(e) = limits.check_key(key.len() as u64) {
        return S3Reply::Error {
            code: "limit_exceeded".into(),
            message: e.to_string(),
        };
    }
    if let Err(e) = limits.check_value(body.len() as u64) {
        return S3Reply::Error {
            code: "limit_exceeded".into(),
            message: e.to_string(),
        };
    }
    if !session.bucket_exists(bucket) {
        if let Err(e) = session.ensure_bucket(bucket, None) {
            return e;
        }
    }
    if meta.etag.is_empty() {
        meta.etag = single_part_etag(&body);
    }
    if meta.last_modified == 0 {
        meta.last_modified = crate::sigv4::now_unix();
    }
    let stored = meta::store_value(&meta, &body);
    match session.exec(LogicalRequest::Object {
        container: bucket.into(),
        key: key.into(),
        bytes: stored,
    }) {
        Ok(_) => S3Reply::OkHeaders(vec![("ETag".into(), meta.etag.clone())]),
        Err(e) => e,
    }
}

/// `x-amz-acl` and `x-amz-grant-*` are ignored. They do not widen authz or
/// cross-namespace visibility; canned ACL subresources stay MustNot (T079).

pub fn stage_object_chunk(
    session: &mut SessionState,
    bucket: &str,
    staging: &str,
    index: u32,
    bytes: Vec<u8>,
) -> Result<(), S3Reply> {
    if bytes.len() > crate::stream::CHUNK_WINDOW {
        return Err(S3Reply::Error {
            code: "InternalError".into(),
            message: "chunk exceeds pump window".into(),
        });
    }
    if !session.bucket_exists(bucket) {
        session.ensure_bucket(bucket, None)?;
    }
    let stored = meta::store_value(&ObjectMeta::default(), &bytes);
    session
        .exec(LogicalRequest::Object {
            container: bucket.into(),
            key: crate::stream::chunk_object_key(staging, index),
            bytes: stored,
        })
        .map(|_| ())
}

pub fn abort_staged_chunks(session: &mut SessionState, bucket: &str, staging: &str, count: u32) {
    for index in 0..count {
        let _ = session.exec(LogicalRequest::Mutate {
            container: bucket.into(),
            op: "delete".into(),
            sql: crate::stream::chunk_object_key(staging, index),
        });
    }
}

pub fn commit_staged_object(
    session: &mut SessionState,
    bucket: &str,
    key: &str,
    meta: ObjectMeta,
    staging: &str,
    total: u64,
    window: usize,
    chunks: u32,
) -> S3Reply {
    if let Err(e) = session.limit_services.limits().check_value(total) {
        abort_staged_chunks(session, bucket, staging, chunks);
        return S3Reply::Error {
            code: "limit_exceeded".into(),
            message: e.to_string(),
        };
    }
    if let Err(e) = session.limit_services.limits().check_key(key.len() as u64) {
        abort_staged_chunks(session, bucket, staging, chunks);
        return S3Reply::Error {
            code: "limit_exceeded".into(),
            message: e.to_string(),
        };
    }
    let previous = load_stored(session, bucket, key);
    let manifest = crate::stream::encode_manifest(staging, total, window, chunks);
    let reply = put_object_with_meta(session, bucket, key, manifest, meta);
    if !matches!(
        reply,
        S3Reply::Ok | S3Reply::OkHeaders(_) | S3Reply::NoContent
    ) {
        abort_staged_chunks(session, bucket, staging, chunks);
        return reply;
    }
    if let Some((_, old_body)) = previous {
        if let Some((old_staging, _, _, old_chunks)) = crate::stream::parse_manifest(&old_body) {
            if old_staging != staging {
                abort_staged_chunks(session, bucket, &old_staging, old_chunks);
            }
        }
    }
    reply
}

pub fn read_staged_chunk(
    session: &mut SessionState,
    bucket: &str,
    staging: &str,
    index: u32,
) -> Result<Vec<u8>, S3Reply> {
    match session.exec(LogicalRequest::Point {
        container: bucket.into(),
        key: crate::stream::chunk_object_key(staging, index),
    }) {
        Ok(r) if r.rows.is_empty() => Err(S3Reply::Error {
            code: "InternalError".into(),
            message: format!("missing chunk {index}"),
        }),
        Ok(r) => {
            let raw = r.rows[0]
                .get(1)
                .and_then(|x| x.as_ref())
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_default();
            let (_, body) = meta::load_value(&raw);
            Ok(body)
        }
        Err(e) => Err(e),
    }
}

fn load_stored(
    session: &mut SessionState,
    bucket: &str,
    key: &str,
) -> Option<(ObjectMeta, Vec<u8>)> {
    let raw = session
        .exec(LogicalRequest::Point {
            container: bucket.into(),
            key: key.into(),
        })
        .ok()?;
    if raw.rows.is_empty() {
        return None;
    }
    let bytes = raw.rows[0]
        .get(1)
        .and_then(|x| x.as_ref())
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_default();
    Some(meta::load_value(&bytes))
}

fn etag_matches(stored: &str, presented: &str) -> bool {
    let norm = |s: &str| s.trim().trim_matches('"').to_string();
    norm(stored).eq_ignore_ascii_case(&norm(presented))
}

fn merged_multipart_uploads(
    session: &SessionState,
    bucket: &str,
) -> Result<Vec<(String, String, String)>, S3Reply> {
    let mut uploads = session
        .multipart
        .read()
        .expect("multipart")
        .list_for_bucket(bucket, &session.namespace);
    let mut seen: std::collections::HashSet<String> =
        uploads.iter().map(|(_, id, _)| id.clone()).collect();
    for (key, upload_id, initiator) in list_persisted_uploads(session, bucket)? {
        if seen.insert(upload_id.clone()) {
            uploads.push((key, upload_id, initiator));
        }
    }
    Ok(uploads)
}

fn list_persisted_uploads(
    session: &SessionState,
    bucket: &str,
) -> Result<Vec<(String, String, String)>, S3Reply> {
    let keys = match session.user_keys(UPLOAD_CONTAINER) {
        Ok(k) => k,
        Err(S3Reply::Error { code, .. }) if code == "NoSuchBucket" => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for key in keys {
        let Some(upload_id) = key.strip_prefix("m/") else {
            continue;
        };
        if session
            .multipart
            .read()
            .expect("multipart")
            .contains(upload_id)
        {
            continue;
        }
        let r = session.exec(LogicalRequest::Point {
            container: UPLOAD_CONTAINER.into(),
            key: format!("m/{upload_id}"),
        })?;
        let raw = r
            .rows
            .first()
            .and_then(|row| row.get(1))
            .and_then(|x| x.clone())
            .unwrap_or_default()
            .into_bytes();
        let (_, meta_body) = meta::load_value(&raw);
        let Some((id, namespace, upload_bucket, object_key)) =
            MultipartRegistry::decode_meta_fields(&meta_body)
        else {
            continue;
        };
        if upload_bucket != bucket || namespace != session.namespace {
            continue;
        }
        out.push((object_key, id, String::new()));
    }
    Ok(out)
}

const UPLOAD_CONTAINER: &str = "__s3_uploads";

/// This process answers catalog reads from one local replica. A session read
/// quorum that needs more acknowledgements cannot be met (FR-009).
fn session_read_quorum_met(level: &str) -> bool {
    const LIVE: u32 = 1;
    let required = match level.trim().to_ascii_uppercase().as_str() {
        "ONE" | "LOCAL_ONE" => 1u32,
        "TWO" => 2,
        "QUORUM" => LIVE / 2 + 1,
        "ALL" => LIVE,
        other => {
            if let Some(rest) = other
                .strip_prefix("ACKS(")
                .and_then(|r| r.strip_suffix(')'))
            {
                match rest.parse::<u32>() {
                    Ok(n) => n,
                    Err(_) => return false,
                }
            } else {
                return false;
            }
        }
    };
    required > 0 && LIVE >= required
}

fn require_read_quorum(session: &SessionState) -> Result<(), S3Reply> {
    let level = session.query_options.quorum_read.value().as_str();
    if session_read_quorum_met(level) {
        Ok(())
    } else {
        Err(S3Reply::Error {
            code: "QuorumUnsatisfiable".into(),
            message: format!("quorum_unsatisfiable:{level}"),
        })
    }
}

fn piece_count(size: u64) -> u32 {
    if size == 0 {
        0
    } else {
        size.div_ceil(crate::stream::CHUNK_WINDOW as u64) as u32
    }
}

fn part_piece_key(upload_id: &str, part_number: u32, index: u32) -> String {
    format!("p/{upload_id}/{part_number:08}/{index:08}")
}

/// Registry meta only. Part bodies are already stored as chunk pieces.
/// Success means the durable registry write committed (T096).
fn persist_upload(session: &mut SessionState, upload_id: &str) -> Result<(), S3Reply> {
    let snapshot = {
        let mp = session.multipart.read().expect("multipart");
        mp.uploads_snapshot(upload_id)
    };
    let Some(upload) = snapshot else {
        return Err(S3Reply::Error {
            code: "NoSuchUpload".into(),
            message: upload_id.into(),
        });
    };
    session.ensure_bucket(UPLOAD_CONTAINER, Some("ensure_object_collection"))?;
    let meta_bytes = meta::store_value(
        &ObjectMeta::default(),
        &crate::multipart::MultipartRegistry::encode_meta(upload_id, &upload),
    );
    session.exec(LogicalRequest::Object {
        container: UPLOAD_CONTAINER.into(),
        key: format!("m/{upload_id}"),
        bytes: meta_bytes,
    })?;
    Ok(())
}

/// One durable window of an `UploadPart`. Callers hash the part incrementally
/// and must not retain earlier windows.
pub struct OpenUploadPart {
    upload_id: String,
    part_number: u32,
    previous: Vec<crate::multipart::PartRecord>,
    old_size: Option<u64>,
}

pub fn open_upload_part(
    session: &mut SessionState,
    upload_id: &str,
    part_number: u32,
) -> Result<OpenUploadPart, S3Reply> {
    if let Err(e) = session.require_grant(Verb::WRITE) {
        return Err(e);
    }
    match load_upload(session, upload_id)? {
        true => {}
        false => {
            let known = session
                .multipart
                .read()
                .expect("multipart")
                .same_namespace(upload_id, &session.namespace);
            if !known {
                return Err(S3Reply::Error {
                    code: "NoSuchUpload".into(),
                    message: upload_id.into(),
                });
            }
        }
    }
    let (previous, old_size) = {
        let mut mp = session.multipart.write().expect("multipart");
        mp.expire_idle();
        let Some(upload) = mp.get_for(upload_id, &session.namespace) else {
            return Err(S3Reply::Error {
                code: "NoSuchUpload".into(),
                message: upload_id.into(),
            });
        };
        if upload.parts.len() >= session.multipart_max_parts as usize
            && !upload.parts.iter().any(|p| p.number == part_number)
        {
            return Err(S3Reply::Error {
                code: "InvalidRequest".into(),
                message: "max parts exceeded".into(),
            });
        }
        let old_size = upload
            .parts
            .iter()
            .find(|p| p.number == part_number)
            .map(|p| p.size);
        (upload.parts.clone(), old_size)
    };
    Ok(OpenUploadPart {
        upload_id: upload_id.to_string(),
        part_number,
        previous,
        old_size,
    })
}

pub fn append_upload_part_window(
    session: &mut SessionState,
    part: &OpenUploadPart,
    index: u32,
    window: Vec<u8>,
) -> Result<(), S3Reply> {
    if window.is_empty() {
        return Ok(());
    }
    if index == 0 {
        session.ensure_bucket(UPLOAD_CONTAINER, Some("ensure_object_collection"))?;
    }
    let stored = meta::store_value(&ObjectMeta::default(), &window);
    session.exec(LogicalRequest::Object {
        container: UPLOAD_CONTAINER.into(),
        key: part_piece_key(&part.upload_id, part.part_number, index),
        bytes: stored,
    })?;
    Ok(())
}

pub fn finish_upload_part(
    session: &mut SessionState,
    part: OpenUploadPart,
    etag: String,
    size: u64,
) -> S3Reply {
    if let Some(old_size) = part.old_size {
        let new_n = piece_count(size);
        for index in new_n..piece_count(old_size) {
            let _ = session.exec(LogicalRequest::Mutate {
                container: UPLOAD_CONTAINER.into(),
                op: "delete".into(),
                sql: part_piece_key(&part.upload_id, part.part_number, index),
            });
        }
    }
    {
        let mut mp = session.multipart.write().expect("multipart");
        let Some(upload) = mp.get_for(&part.upload_id, &session.namespace) else {
            return S3Reply::Error {
                code: "NoSuchUpload".into(),
                message: part.upload_id,
            };
        };
        if let Some(existing) = upload
            .parts
            .iter_mut()
            .find(|p| p.number == part.part_number)
        {
            existing.etag = etag.clone();
            existing.size = size;
        } else {
            upload.parts.push(crate::multipart::PartRecord {
                number: part.part_number,
                etag: etag.clone(),
                size,
            });
        }
    }
    if let Err(e) = persist_upload(session, &part.upload_id) {
        if let Some(upload) = session
            .multipart
            .write()
            .expect("multipart")
            .get_for(&part.upload_id, &session.namespace)
        {
            upload.parts = part.previous;
        }
        return e;
    }
    S3Reply::OkHeaders(vec![("ETag".into(), etag)])
}

fn read_part_piece(
    session: &mut SessionState,
    upload_id: &str,
    part_number: u32,
    index: u32,
) -> Result<Vec<u8>, S3Reply> {
    match session.exec(LogicalRequest::Point {
        container: UPLOAD_CONTAINER.into(),
        key: part_piece_key(upload_id, part_number, index),
    }) {
        Ok(r) if r.rows.is_empty() => Err(S3Reply::Error {
            code: "InternalError".into(),
            message: format!("missing part {part_number} piece {index}"),
        }),
        Ok(r) => {
            let raw = r.rows[0]
                .get(1)
                .and_then(|x| x.as_ref())
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_default();
            let (_, body) = meta::load_value(&raw);
            Ok(body)
        }
        Err(e) => Err(e),
    }
}

/// Publish a completed object from stored part pieces. Objects larger than
/// `CHUNK_WINDOW` become a manifest of those pieces; smaller ones are one
/// window read from storage. Neither path keeps `PartRecord` bodies.
fn publish_completed_object(
    session: &mut SessionState,
    bucket: &str,
    key: &str,
    meta: ObjectMeta,
    upload_id: &str,
    parts: &[crate::multipart::PartRecord],
    total: u64,
) -> S3Reply {
    if total > crate::stream::CHUNK_WINDOW as u64 {
        let staging = uuid_simple();
        let mut index = 0u32;
        for part in parts {
            for ci in 0..piece_count(part.size) {
                let bytes = match read_part_piece(session, upload_id, part.number, ci) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        abort_staged_chunks(session, bucket, &staging, index);
                        return e;
                    }
                };
                if let Err(e) = stage_object_chunk(session, bucket, &staging, index, bytes) {
                    abort_staged_chunks(session, bucket, &staging, index);
                    return e;
                }
                index += 1;
            }
        }
        return commit_staged_object(
            session,
            bucket,
            key,
            meta,
            &staging,
            total,
            crate::stream::CHUNK_WINDOW,
            index,
        );
    }
    let mut assembled = Vec::new();
    for part in parts {
        for ci in 0..piece_count(part.size) {
            match read_part_piece(session, upload_id, part.number, ci) {
                Ok(bytes) => assembled.extend_from_slice(&bytes),
                Err(e) => return e,
            }
        }
    }
    put_object_with_meta(session, bucket, key, assembled, meta)
}

/// `Ok(true)` when the upload is in memory or was loaded from durable storage.
/// `Ok(false)` when the durable row is absent for this namespace.
/// Registry `Point` / listing errors fail closed (FR-009).
fn load_upload(session: &mut SessionState, upload_id: &str) -> Result<bool, S3Reply> {
    {
        let mp = session.multipart.read().expect("multipart");
        if mp.same_namespace(upload_id, &session.namespace) {
            return Ok(true);
        }
    }
    require_read_quorum(session)?;
    if !session.bucket_exists(UPLOAD_CONTAINER) {
        return Ok(false);
    }
    let r = session.exec(LogicalRequest::Point {
        container: UPLOAD_CONTAINER.into(),
        key: format!("m/{upload_id}"),
    })?;
    if r.rows.is_empty() {
        return Ok(false);
    }
    let meta = r.rows[0]
        .get(1)
        .and_then(|x| x.clone())
        .unwrap_or_default()
        .into_bytes();
    let (_, meta_body) = meta::load_value(&meta);
    let Some((id, upload)) = crate::multipart::MultipartRegistry::decode_meta(&meta_body) else {
        return Ok(false);
    };
    if upload.namespace != session.namespace {
        return Ok(false);
    }
    session
        .multipart
        .write()
        .expect("multipart")
        .insert_loaded(id, upload);
    Ok(true)
}

fn delete_persisted_upload(
    session: &mut SessionState,
    upload_id: &str,
    parts: &[(u32, u64)],
) -> Result<(), S3Reply> {
    session.exec(LogicalRequest::Mutate {
        container: UPLOAD_CONTAINER.into(),
        op: "delete".into(),
        sql: format!("m/{upload_id}"),
    })?;
    for (n, size) in parts {
        for index in 0..piece_count(*size) {
            session.exec(LogicalRequest::Mutate {
                container: UPLOAD_CONTAINER.into(),
                op: "delete".into(),
                sql: part_piece_key(upload_id, *n, index),
            })?;
        }
    }
    Ok(())
}

pub fn upload_part_bytes(
    session: &mut SessionState,
    upload_id: &str,
    part_number: u32,
    body: Vec<u8>,
) -> S3Reply {
    if let Err(e) = session.require_grant(Verb::WRITE) {
        return e;
    }
    match load_upload(session, upload_id) {
        Ok(true) => {}
        Ok(false) => {
            let known = session
                .multipart
                .read()
                .expect("multipart")
                .same_namespace(upload_id, &session.namespace);
            if !known {
                return S3Reply::Error {
                    code: "NoSuchUpload".into(),
                    message: upload_id.into(),
                };
            }
        }
        Err(e) => return e,
    }
    let etag = single_part_etag(&body);
    let size = body.len() as u64;
    let previous = {
        let mut mp = session.multipart.write().expect("multipart");
        mp.expire_idle();
        let Some(upload) = mp.get_for(upload_id, &session.namespace) else {
            return S3Reply::Error {
                code: "NoSuchUpload".into(),
                message: upload_id.into(),
            };
        };
        if upload.parts.len() >= session.multipart_max_parts as usize
            && !upload.parts.iter().any(|p| p.number == part_number)
        {
            return S3Reply::Error {
                code: "InvalidRequest".into(),
                message: "max parts exceeded".into(),
            };
        }
        let old_size = upload
            .parts
            .iter()
            .find(|p| p.number == part_number)
            .map(|p| p.size);
        (upload.parts.clone(), old_size)
    };
    let mut off = 0;
    let mut index = 0u32;
    while off < body.len() {
        let end = (off + crate::stream::CHUNK_WINDOW).min(body.len());
        if let Err(e) = append_upload_part_window(
            session,
            &OpenUploadPart {
                upload_id: upload_id.to_string(),
                part_number,
                previous: previous.0.clone(),
                old_size: previous.1,
            },
            index,
            body[off..end].to_vec(),
        ) {
            return e;
        }
        index += 1;
        off = end;
    }
    finish_upload_part(
        session,
        OpenUploadPart {
            upload_id: upload_id.to_string(),
            part_number,
            previous: previous.0,
            old_size: previous.1,
        },
        etag,
        size,
    )
}

fn list_keys(session: &SessionState, bucket: &str, prefix: &str) -> Result<Vec<String>, S3Reply> {
    let keys = session.user_keys(bucket)?;
    Ok(keys
        .into_iter()
        .filter(|k| prefix.is_empty() || k.starts_with(prefix))
        .collect())
}

#[derive(Clone)]
struct ListItem {
    name: String,
    etag: String,
    size: u64,
    last_modified: u64,
    is_prefix: bool,
}

fn stored_etag(session: &SessionState, bucket: &str, key: &str) -> Result<String, S3Reply> {
    Ok(stored_object_stat(session, bucket, key)?.0)
}

fn stored_object_stat(
    session: &SessionState,
    bucket: &str,
    key: &str,
) -> Result<(String, u64, u64), S3Reply> {
    require_read_quorum(session)?;
    match session.exec(LogicalRequest::Point {
        container: bucket.into(),
        key: key.into(),
    }) {
        Ok(r) if !r.rows.is_empty() => {
            let raw = r.rows[0]
                .get(1)
                .and_then(|x| x.as_ref())
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_default();
            let (meta, body) = meta::load_value(&raw);
            if let Some((_, total, _, _)) = crate::stream::parse_manifest(&body) {
                let etag = if meta.etag.is_empty() {
                    "\"\"".into()
                } else {
                    meta.etag
                };
                Ok((etag, total, meta.last_modified))
            } else {
                let etag = if meta.etag.is_empty() {
                    single_part_etag(&body)
                } else {
                    meta.etag
                };
                Ok((etag, body.len() as u64, meta.last_modified))
            }
        }
        Ok(_) => Ok((String::new(), 0, 0)),
        Err(e) => Err(e),
    }
}

fn build_listing(
    session: &SessionState,
    bucket: &str,
    keys: Vec<String>,
    prefix: &str,
    delimiter: &str,
) -> Result<Vec<ListItem>, S3Reply> {
    if delimiter.is_empty() {
        let mut items = Vec::with_capacity(keys.len());
        for k in keys {
            let (etag, size, last_modified) = stored_object_stat(session, bucket, &k)?;
            items.push(ListItem {
                name: k,
                etag,
                size,
                last_modified,
                is_prefix: false,
            });
        }
        return Ok(items);
    }
    // Common prefixes are grouped on the first delimiter *after* `prefix`
    // (AWS ListObjects). Grouping from the start of the key hides objects
    // when the client lists `prefix=restic/keys/` with `delimiter=/`.
    let mut prefixes = Vec::new();
    let mut plain = Vec::new();
    for key in keys {
        let rest = key.strip_prefix(prefix).unwrap_or(key.as_str());
        if let Some(idx) = rest.find(delimiter) {
            let cp = format!("{prefix}{}{delimiter}", &rest[..idx]);
            if !prefixes.contains(&cp) {
                prefixes.push(cp);
            }
        } else {
            plain.push(key);
        }
    }
    let mut items = Vec::new();
    for k in plain {
        let (etag, size, last_modified) = stored_object_stat(session, bucket, &k)?;
        items.push(ListItem {
            name: k,
            etag,
            size,
            last_modified,
            is_prefix: false,
        });
    }
    for p in prefixes {
        items.push(ListItem {
            name: p,
            etag: String::new(),
            size: 0,
            last_modified: 0,
            is_prefix: true,
        });
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

fn paginate_items(
    items: &[ListItem],
    max_keys: usize,
    start_after: Option<&str>,
) -> (Vec<ListItem>, bool) {
    let mut filtered: Vec<ListItem> = items
        .iter()
        .filter(|i| start_after.is_none_or(|m| i.name.as_str() > m))
        .cloned()
        .collect();
    filtered.sort_by(|a, b| a.name.cmp(&b.name));
    if filtered.len() <= max_keys {
        (filtered, false)
    } else {
        (filtered[..max_keys].to_vec(), true)
    }
}

const OPTS_KEY: &str = "\0opts";

fn store_bucket_opts(session: &mut SessionState, bucket: &str, opts: &[(String, String)]) {
    let body = opts
        .iter()
        .map(|(k, v)| format!("{k}\t{v}"))
        .collect::<Vec<_>>()
        .join("\n");
    let stored = meta::store_value(&ObjectMeta::default(), body.as_bytes());
    let _ = session.exec(LogicalRequest::Object {
        container: bucket.into(),
        key: OPTS_KEY.into(),
        bytes: stored,
    });
}

fn bucket_opt_headers(session: &SessionState, bucket: &str) -> Vec<(String, String)> {
    let raw = match session.exec(LogicalRequest::Point {
        container: bucket.into(),
        key: OPTS_KEY.into(),
    }) {
        Ok(r) if !r.rows.is_empty() => r.rows[0]
            .get(1)
            .and_then(|x| x.as_ref())
            .map(|s| s.as_bytes().to_vec())
            .unwrap_or_default(),
        _ => return Vec::new(),
    };
    let (_, body) = meta::load_value(&raw);
    let text = String::from_utf8_lossy(&body);
    text.lines()
        .filter_map(|line| {
            let (k, v) = line.split_once('\t')?;
            if k.is_empty() {
                return None;
            }
            Some((format!("x-spacestorage-opt-{k}"), v.to_string()))
        })
        .collect()
}

fn uuid_simple() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    format!("{}", SEQ.fetch_add(1, Ordering::Relaxed))
}

pub fn copy_object(
    session: &mut SessionState,
    dest_bucket: &str,
    dest_key: &str,
    copy_source: &str,
    directive: &str,
    replace: &ObjectMeta,
) -> S3Reply {
    let trimmed = copy_source.trim().trim_start_matches('/');
    let Some((src_bucket, src_key)) = trimmed.split_once('/') else {
        return S3Reply::Error {
            code: "InvalidRequest".into(),
            message: "bad x-amz-copy-source".into(),
        };
    };
    if src_bucket != dest_bucket && !session.bucket_exists(src_bucket) {
        return S3Reply::Error {
            code: "NoSuchBucket".into(),
            message: src_bucket.into(),
        };
    }
    let (bytes, meta) = match dispatch(session, "GETOBJECT", &[src_bucket, src_key]) {
        S3Reply::Body(obj) => {
            let meta = if directive.eq_ignore_ascii_case("REPLACE") {
                let mut m = replace.clone();
                m.etag.clear();
                m
            } else {
                header_meta(&obj.headers)
            };
            (obj.bytes, meta)
        }
        S3Reply::ChunkedObject {
            bucket: src_bucket_name,
            staging,
            total,
            chunk_count,
            window,
            headers,
            ..
        } => {
            let meta = if directive.eq_ignore_ascii_case("REPLACE") {
                let mut m = replace.clone();
                m.etag.clear();
                m
            } else {
                header_meta(&headers)
            };
            let new_staging = uuid_simple();
            for index in 0..chunk_count {
                let chunk = match read_staged_chunk(session, &src_bucket_name, &staging, index) {
                    Ok(c) => c,
                    Err(e) => return e,
                };
                if let Err(e) = stage_object_chunk(session, dest_bucket, &new_staging, index, chunk)
                {
                    abort_staged_chunks(session, dest_bucket, &new_staging, index);
                    return e;
                }
            }
            return match commit_staged_object(
                session,
                dest_bucket,
                dest_key,
                meta,
                &new_staging,
                total,
                window,
                chunk_count,
            ) {
                S3Reply::OkHeaders(headers) => {
                    let etag = headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    S3Reply::Xml(format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CopyObjectResult><ETag>{}</ETag></CopyObjectResult>",
                        crate::xml::xml_escape(&etag)
                    ))
                }
                other => other,
            };
        }
        S3Reply::Error { code, message } if code == "NoSuchKey" => {
            return S3Reply::Error { code, message };
        }
        other => return other,
    };
    let dest_etag = meta.etag.clone();
    match put_object_with_meta(session, dest_bucket, dest_key, bytes, meta) {
        S3Reply::OkHeaders(headers) => {
            let etag = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
                .map(|(_, v)| v.clone())
                .unwrap_or(dest_etag);
            S3Reply::Xml(format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CopyObjectResult><ETag>{}</ETag></CopyObjectResult>",
                crate::xml::xml_escape(&etag)
            ))
        }
        S3Reply::Ok => {
            let etag = match stored_etag(session, dest_bucket, dest_key) {
                Ok(etag) => etag,
                Err(e) => return e,
            };
            S3Reply::Xml(format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CopyObjectResult><ETag>{}</ETag></CopyObjectResult>",
                crate::xml::xml_escape(&etag)
            ))
        }
        other => other,
    }
}

fn header_meta(headers: &[(String, String)]) -> ObjectMeta {
    let mut meta = ObjectMeta::default();
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-type") {
            meta.content_type = v.clone();
        } else if k.eq_ignore_ascii_case("cache-control") {
            meta.cache_control = v.clone();
        } else if k.eq_ignore_ascii_case("content-disposition") {
            meta.content_disposition = v.clone();
        } else if k.eq_ignore_ascii_case("content-encoding") {
            meta.content_encoding = v.clone();
        } else if k.eq_ignore_ascii_case("etag") {
            meta.etag = v.clone();
        } else if let Some(name) = k
            .strip_prefix("x-amz-meta-")
            .or_else(|| k.strip_prefix("X-Amz-Meta-"))
        {
            meta.user_meta.push((name.to_string(), v.clone()));
        }
    }
    meta
}

fn map_type_op(name: &str) -> String {
    match name.trim() {
        "kv" | "kv_store" | "kv_collection" | "K/V Store" => "ensure_kv_store".into(),
        "document_store" | "document" | "Document Store" => "ensure_document_store".into(),
        "relational_table" | "Relational Table" => "ensure_relational_table".into(),
        "object_collection" | "Object Collection" | "" => "ensure_object_collection".into(),
        other => format!("ensure_{other}"),
    }
}

fn grant_for(op: &str) -> Verb {
    match op {
        "LISTBUCKETS"
        | "HEADBUCKET"
        | "GETBUCKETLOCATION"
        | "LISTOBJECTSV2"
        | "LISTOBJECTS"
        | "GETOBJECT"
        | "HEADOBJECT"
        | "LISTPARTS"
        | "LISTMULTIPARTUPLOADS" => Verb::READ,
        "CREATEBUCKET" => Verb::CREATE,
        "DELETEBUCKET" => Verb::DROP,
        _ => Verb::WRITE,
    }
}

pub fn dispatch(session: &mut SessionState, verb: &str, args: &[&str]) -> S3Reply {
    let upper = verb.to_ascii_uppercase().replace('-', "_");
    match classify_outcome(session.profile, ProtocolId::S3, &upper) {
        ClassifyOutcome::Must => {}
        ClassifyOutcome::MustNot(err) => {
            if matches!(upper.as_str(), "UPLOADPARTCOPY" | "GETOBJECTATTRIBUTES") {
                return S3Reply::Error {
                    code: "NotImplemented".into(),
                    message: format!("{upper} is not yet implemented"),
                };
            }
            return refuse(&upper, err);
        }
    }

    if !session.namespace_bound && upper != "LISTBUCKETS" {
        return S3Reply::Error {
            code: "AccessDenied".into(),
            message: "UnboundCredential".into(),
        };
    }

    if let Err(e) = session.require_grant(grant_for(&upper)) {
        return e;
    }

    match upper.as_str() {
        "LISTBUCKETS" => {
            if !session.namespace_bound {
                return S3Reply::Error {
                    code: "AccessDenied".into(),
                    message: "UnboundCredential".into(),
                };
            }
            let cat = session.catalog.read().expect("catalog");
            let buckets: Vec<(String, String)> = cat
                .list()
                .filter(|c| c.namespace.as_str() == session.namespace)
                .filter(|c| !c.name.as_str().starts_with("__"))
                .map(|c| {
                    let raw = c.name.as_str();
                    (s3_bucket_list_name(raw), c.model.machine_name().to_string())
                })
                .collect();
            S3Reply::Xml(list_buckets_xml(&buckets))
        }
        "HEADBUCKET" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            if session.bucket_exists(bucket) {
                S3Reply::Head {
                    content_length: 0,
                    headers: bucket_opt_headers(session, bucket),
                }
            } else {
                S3Reply::Error {
                    code: "NoSuchBucket".into(),
                    message: bucket.into(),
                }
            }
        }
        "CREATEBUCKET" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            if !s3_bucket_name_valid(bucket) {
                return S3Reply::Error {
                    code: "InvalidBucketName".into(),
                    message: bucket.into(),
                };
            }
            if session.bucket_exists(bucket) {
                return S3Reply::Error {
                    code: "BucketAlreadyOwnedByYou".into(),
                    message: bucket.into(),
                };
            }
            let requested = session.requested_type.take();
            let opts = std::mem::take(&mut session.requested_opts);
            let op = requested
                .as_deref()
                .map(map_type_op)
                .unwrap_or_else(|| session.bucket_type_op.clone());
            match session.ensure_bucket(bucket, Some(&op)) {
                Ok(()) => {
                    if !opts.is_empty() {
                        store_bucket_opts(session, bucket, &opts);
                    }
                    S3Reply::Ok
                }
                Err(e) => e,
            }
        }
        "DELETEBUCKET" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            match session.bucket_empty(bucket) {
                Ok(true) => match session.exec(LogicalRequest::TypeOp {
                    container: bucket.into(),
                    op: "drop".into(),
                }) {
                    Ok(_) => S3Reply::NoContent,
                    Err(e) => e,
                },
                Ok(false) => S3Reply::Error {
                    code: "BucketNotEmpty".into(),
                    message: bucket.into(),
                },
                Err(e) => e,
            }
        }
        "GETBUCKETLOCATION" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            if !session.bucket_exists(bucket) {
                return S3Reply::Error {
                    code: "NoSuchBucket".into(),
                    message: bucket.into(),
                };
            }
            S3Reply::Xml(bucket_location_xml(&session.region))
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
            if !session.bucket_exists(bucket) {
                return S3Reply::Error {
                    code: "NoSuchBucket".into(),
                    message: bucket.into(),
                };
            }
            if let Err(e) = require_read_quorum(session) {
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
                    let raw = r.rows[0]
                        .get(1)
                        .and_then(|x| x.as_ref())
                        .map(|s| s.as_bytes().to_vec())
                        .unwrap_or_default();
                    let (mut meta, body) = meta::load_value(&raw);
                    if let Some((staging, total, window, chunks)) =
                        crate::stream::parse_manifest(&body)
                    {
                        if meta.etag.is_empty() {
                            meta.etag = "\"\"".into();
                        }
                        let headers = meta::response_headers(&meta, total);
                        if upper == "HEADOBJECT" {
                            S3Reply::Head {
                                content_length: total,
                                headers,
                            }
                        } else {
                            S3Reply::ChunkedObject {
                                bucket: bucket.to_string(),
                                key: key.to_string(),
                                staging,
                                total,
                                chunk_count: chunks,
                                window,
                                headers,
                                status: 200,
                                range: None,
                            }
                        }
                    } else {
                        if meta.etag.is_empty() {
                            meta.etag = single_part_etag(&body);
                        }
                        let headers = meta::response_headers(&meta, body.len() as u64);
                        if upper == "HEADOBJECT" {
                            S3Reply::Head {
                                content_length: body.len() as u64,
                                headers,
                            }
                        } else {
                            S3Reply::Body(ObjectReply {
                                bytes: body,
                                status: 200,
                                headers,
                            })
                        }
                    }
                }
                Err(e) => e,
            }
        }
        "DELETEOBJECT" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let key = args.get(1).copied().unwrap_or("key");
            if !session.bucket_exists(bucket) {
                return S3Reply::Error {
                    code: "NoSuchBucket".into(),
                    message: bucket.into(),
                };
            }
            let staged = load_stored(session, bucket, key).and_then(|(_, body)| {
                crate::stream::parse_manifest(&body)
                    .map(|(staging, _, _, chunks)| (staging, chunks))
            });
            if let Err(e) = session.exec(LogicalRequest::Mutate {
                container: bucket.into(),
                op: "delete".into(),
                sql: key.into(),
            }) {
                return e;
            }
            if let Some((staging, chunks)) = staged {
                abort_staged_chunks(session, bucket, &staging, chunks);
            }
            S3Reply::NoContent
        }
        "LISTOBJECTSV2" | "LISTOBJECTS" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let prefix = args.get(1).copied().unwrap_or("");
            let (start_after, max_keys) = if upper == "LISTOBJECTS" {
                let marker = args.get(2).filter(|s| !s.is_empty()).map(|s| *s);
                let max_keys = args
                    .get(3)
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|n| *n > 0)
                    .unwrap_or(1000);
                (marker, max_keys)
            } else {
                let max_keys = args
                    .get(2)
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|n| *n > 0)
                    .unwrap_or(1000);
                let token = args.get(3).filter(|s| !s.is_empty()).map(|s| *s);
                let start_after = args.get(6).filter(|s| !s.is_empty()).map(|s| *s);
                (token.or(start_after), max_keys)
            };
            if !session.bucket_exists(bucket) {
                return S3Reply::Error {
                    code: "NoSuchBucket".into(),
                    message: bucket.into(),
                };
            }
            let keys = match list_keys(session, bucket, prefix) {
                Ok(k) => k,
                Err(e) => return e,
            };
            let delimiter = args.get(4).copied().unwrap_or("");
            let encoding = args.get(5).map(|s| *s == "url").unwrap_or(false);
            let legacy_v1 = upper == "LISTOBJECTS";
            let items = match build_listing(session, bucket, keys, prefix, delimiter) {
                Ok(items) => items,
                Err(e) => return e,
            };
            let (page, truncated) = paginate_items(&items, max_keys, start_after);
            let cont = if truncated {
                page.last().map(|i| i.name.as_str())
            } else {
                None
            };
            let page_keys: Vec<(String, String, u64, u64)> = page
                .iter()
                .filter(|i| !i.is_prefix)
                .map(|i| (i.name.clone(), i.etag.clone(), i.size, i.last_modified))
                .collect();
            let prefixes: Vec<String> = page
                .iter()
                .filter(|i| i.is_prefix)
                .map(|i| i.name.clone())
                .collect();
            S3Reply::Xml(list_objects_v2_xml(
                bucket, &page_keys, &prefixes, truncated, cont, encoding, legacy_v1,
            ))
        }
        "CREATEMULTIPARTUPLOAD" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let key = args.get(1).copied().unwrap_or("key");
            let _ = session.ensure_bucket(bucket, None);
            let upload_id = format!("mpu-{}", uuid_simple());
            {
                session.multipart.write().expect("multipart").create(
                    upload_id.clone(),
                    bucket.into(),
                    key.into(),
                    session.namespace.clone(),
                );
            }
            match persist_upload(session, &upload_id) {
                Ok(()) => S3Reply::Xml(initiate_multipart_xml(&upload_id)),
                Err(e) => {
                    session
                        .multipart
                        .write()
                        .expect("multipart")
                        .remove_for(&upload_id, &session.namespace);
                    e
                }
            }
        }
        "UPLOADPART" => {
            let upload_id = args.first().copied().unwrap_or("");
            let part_number = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
            let body = args.get(2).copied().unwrap_or("").as_bytes().to_vec();
            upload_part_bytes(session, upload_id, part_number, body)
        }
        "COMPLETEMULTIPARTUPLOAD" => {
            let upload_id = args.first().copied().unwrap_or("");
            let complete_xml = args.get(1).copied().unwrap_or("");
            match load_upload(session, upload_id) {
                Ok(true) => {}
                Ok(false) => {
                    return S3Reply::Error {
                        code: "NoSuchUpload".into(),
                        message: upload_id.into(),
                    };
                }
                Err(e) => return e,
            }
            let upload = {
                let mut mp = session.multipart.write().expect("multipart");
                mp.expire_idle();
                mp.get_for(upload_id, &session.namespace).cloned()
            };
            let Some(upload) = upload else {
                return S3Reply::Error {
                    code: "NoSuchUpload".into(),
                    message: upload_id.into(),
                };
            };
            let by_number: std::collections::HashMap<u32, crate::multipart::PartRecord> =
                upload.parts.into_iter().map(|p| (p.number, p)).collect();

            let selected: Vec<crate::multipart::PartRecord> = if complete_xml.is_empty() {
                let mut parts: Vec<_> = by_number.into_values().collect();
                parts.sort_by_key(|p| p.number);
                parts
            } else if let Some(listed) = parse_complete_multipart_parts(complete_xml) {
                let mut prev = 0u32;
                let mut picked = Vec::with_capacity(listed.len());
                for (number, etag) in listed {
                    if number <= prev {
                        return S3Reply::Error {
                            code: "InvalidPartOrder".into(),
                            message: "parts out of order".into(),
                        };
                    }
                    prev = number;
                    let Some(part) = by_number.get(&number) else {
                        return S3Reply::Error {
                            code: "InvalidPart".into(),
                            message: format!("missing part {number}"),
                        };
                    };
                    if !etag_matches(&part.etag, &etag) {
                        return S3Reply::Error {
                            code: "InvalidPart".into(),
                            message: format!("etag mismatch for part {number}"),
                        };
                    }
                    picked.push(part.clone());
                }
                picked
            } else {
                return S3Reply::Error {
                    code: "MalformedXML".into(),
                    message: "invalid CompleteMultipartUpload".into(),
                };
            };

            let min = session.multipart_min_part;
            if selected.len() > 1 {
                for part in &selected[..selected.len() - 1] {
                    if part.size < min {
                        return S3Reply::Error {
                            code: "EntityTooSmall".into(),
                            message: format!("part {} below minimum {}", part.number, min),
                        };
                    }
                }
            }

            let total: u64 = selected.iter().map(|p| p.size).sum();
            if let Err(e) = session.limit_services.limits().check_value(total) {
                return S3Reply::Error {
                    code: "limit_exceeded".into(),
                    message: e.to_string(),
                };
            }
            let Some(upload) = session
                .multipart
                .write()
                .expect("multipart")
                .remove_for(upload_id, &session.namespace)
            else {
                return S3Reply::Error {
                    code: "NoSuchUpload".into(),
                    message: upload_id.into(),
                };
            };

            let mut etags = Vec::new();
            let part_ids: Vec<(u32, u64)> = selected.iter().map(|p| (p.number, p.size)).collect();
            for p in &selected {
                etags.push(p.etag.clone());
            }
            let mut meta = ObjectMeta::default();
            let etag = multipart_etag(&etags);
            meta.etag = etag.clone();
            let bucket = upload.bucket.clone();
            let key = upload.key.clone();
            let committed =
                publish_completed_object(session, &bucket, &key, meta, upload_id, &selected, total);
            match committed {
                S3Reply::OkHeaders(_) | S3Reply::Ok => {
                    match delete_persisted_upload(session, upload_id, &part_ids) {
                        Ok(()) => {
                            S3Reply::Xml(complete_multipart_upload_result(&bucket, &key, &etag))
                        }
                        Err(e) => e,
                    }
                }
                other => other,
            }
        }
        "ABORTMULTIPARTUPLOAD" => {
            let upload_id = args.first().copied().unwrap_or("");
            match load_upload(session, upload_id) {
                Ok(true) => {}
                Ok(false) => {
                    return S3Reply::Error {
                        code: "NoSuchUpload".into(),
                        message: upload_id.into(),
                    };
                }
                Err(e) => return e,
            }
            let upload = session
                .multipart
                .write()
                .expect("multipart")
                .get_for(upload_id, &session.namespace)
                .cloned();
            let Some(upload) = upload else {
                return S3Reply::Error {
                    code: "NoSuchUpload".into(),
                    message: upload_id.into(),
                };
            };
            let parts: Vec<(u32, u64)> = upload.parts.iter().map(|p| (p.number, p.size)).collect();
            match delete_persisted_upload(session, upload_id, &parts) {
                Ok(()) => {
                    session
                        .multipart
                        .write()
                        .expect("multipart")
                        .remove_for(upload_id, &session.namespace);
                    S3Reply::NoContent
                }
                Err(e) => e,
            }
        }
        "LISTPARTS" => {
            let upload_id = args.get(2).copied().unwrap_or("");
            match load_upload(session, upload_id) {
                Ok(true) => {}
                Ok(false) => {
                    return S3Reply::Error {
                        code: "NoSuchUpload".into(),
                        message: upload_id.into(),
                    };
                }
                Err(e) => return e,
            }
            let mut mp = session.multipart.write().expect("multipart");
            let Some(upload) = mp.get_for(upload_id, &session.namespace) else {
                return S3Reply::Error {
                    code: "NoSuchUpload".into(),
                    message: upload_id.into(),
                };
            };
            let parts: Vec<(u32, String)> = upload
                .parts
                .iter()
                .map(|p| (p.number, p.etag.clone()))
                .collect();
            S3Reply::Xml(list_parts_xml(upload_id, &parts))
        }
        "LISTMULTIPARTUPLOADS" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let uploads = match merged_multipart_uploads(session, bucket) {
                Ok(u) => u,
                Err(e) => return e,
            };
            S3Reply::Xml(list_multipart_uploads_xml(bucket, &uploads))
        }
        "DELETEOBJECTS" => {
            let bucket = args.first().copied().unwrap_or("bucket");
            let body = args.get(1).copied().unwrap_or("");
            let (keys, quiet) = parse_delete_keys(body);
            let mut deleted = Vec::new();
            let mut errors = Vec::new();
            for key in &keys {
                match dispatch(session, "DELETEOBJECT", &[bucket, key]) {
                    S3Reply::NoContent => deleted.push(key.as_str()),
                    S3Reply::Error { code, message } => {
                        errors.push((key.clone(), code, message));
                    }
                    other => {
                        errors.push((
                            key.clone(),
                            "InternalError".into(),
                            format!("unexpected delete reply: {other:?}"),
                        ));
                    }
                }
            }
            S3Reply::Xml(delete_objects_result_xml(&deleted, &errors, quiet))
        }
        "COPYOBJECT" => {
            let dest_bucket = args.first().copied().unwrap_or("bucket");
            let dest_key = args.get(1).copied().unwrap_or("key");
            let copy_source = args.get(2).copied().unwrap_or("");
            copy_object(
                session,
                dest_bucket,
                dest_key,
                copy_source,
                "COPY",
                &ObjectMeta::default(),
            )
        }
        "PRESIGNED" => S3Reply::Error {
            code: "NotImplemented".into(),
            message: "presigned dispatch internal".into(),
        },
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
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(
            dispatch(&mut s, "CREATEBUCKET", &["bbb"]),
            S3Reply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "PUTOBJECT", &["bbb", "k", "v"]),
            S3Reply::OkHeaders(_)
        ));
        match dispatch(&mut s, "GETOBJECT", &["bbb", "k"]) {
            S3Reply::Body(b) => assert_eq!(b.bytes, b"v"),
            other => panic!("{other:?}"),
        }
        let model = cat.read().unwrap().describe("demo", "bbb").unwrap().model;
        assert_eq!(model, spacestorage_types::L3Model::ObjectCollection);
        match dispatch(&mut s, "CREATEMULTIPARTUPLOAD", &["bbb", "m"]) {
            S3Reply::Xml(x) => assert!(x.contains("UploadId")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn delete_empty_bucket_ok() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATEBUCKET", &["bbb"]),
            S3Reply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "DELETEBUCKET", &["bbb"]),
            S3Reply::NoContent
        ));
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

    #[test]
    fn unbound_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        s.namespace_bound = false;
        match dispatch(&mut s, "PUTOBJECT", &["b", "k", "v"]) {
            S3Reply::Error { code, .. } => assert_eq!(code, "AccessDenied"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn invalid_bucket_name_on_create() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        match dispatch(&mut s, "CREATEBUCKET", &["ab"]) {
            S3Reply::Error { code, .. } => assert_eq!(code, "InvalidBucketName"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn list_buckets_escapes_nonconforming_names() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat));
        let _ = dispatch(&mut s, "PUTOBJECT", &["ab", "k", "v"]);
        match dispatch(&mut s, "LISTBUCKETS", &[]) {
            S3Reply::Xml(x) => assert!(x.contains("x--6162"), "{x}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multipart_ack_requires_durable_registry_write() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(
            dispatch(&mut s, "CREATEBUCKET", &["bbb"]),
            S3Reply::Ok
        ));
        cat.write().expect("catalog").set_durable_hooks(
            Some(Arc::new(|_, _, key, _| {
                if key.starts_with("m/") {
                    Err("quorum not met".into())
                } else {
                    Ok(())
                }
            })),
            None,
        );
        match dispatch(&mut s, "CREATEMULTIPARTUPLOAD", &["bbb", "m"]) {
            S3Reply::Error { code, .. } => assert_eq!(code, "QuorumUnsatisfiable"),
            other => panic!("create must not ack an in-memory upload: {other:?}"),
        }
        match dispatch(&mut s, "LISTMULTIPARTUPLOADS", &["bbb"]) {
            S3Reply::Xml(x) => assert!(!x.contains("<Upload>"), "{x}"),
            other => panic!("{other:?}"),
        }

        cat.write().expect("catalog").set_durable_hooks(None, None);
        let init = dispatch(&mut s, "CREATEMULTIPARTUPLOAD", &["bbb", "m"]);
        let S3Reply::Xml(xml) = init else {
            panic!("{init:?}");
        };
        let upload_id = xml
            .split("<UploadId>")
            .nth(1)
            .and_then(|r| r.split("</UploadId>").next())
            .unwrap();
        cat.write().expect("catalog").set_durable_hooks(
            Some(Arc::new(|_, _, key, _| {
                if key.starts_with("m/") {
                    Err("quorum not met".into())
                } else {
                    Ok(())
                }
            })),
            None,
        );
        match dispatch(&mut s, "UPLOADPART", &[upload_id, "1", "hello"]) {
            S3Reply::Error { code, .. } => assert_eq!(code, "QuorumUnsatisfiable"),
            other => panic!("upload part must not ack: {other:?}"),
        }
        match dispatch(&mut s, "LISTPARTS", &["bbb", "m", upload_id]) {
            S3Reply::Xml(x) => assert!(!x.contains("<PartNumber>"), "{x}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multipart_complete_reads_stored_pieces() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat)).with_multipart_limits(1, 100);
        assert!(matches!(
            dispatch(&mut s, "CREATEBUCKET", &["bbb"]),
            S3Reply::Ok
        ));
        let init = dispatch(&mut s, "CREATEMULTIPARTUPLOAD", &["bbb", "obj"]);
        let S3Reply::Xml(xml) = init else {
            panic!("{init:?}");
        };
        let upload_id = xml
            .split("<UploadId>")
            .nth(1)
            .and_then(|r| r.split("</UploadId>").next())
            .unwrap()
            .to_string();
        let big = vec![b'a'; crate::stream::CHUNK_WINDOW + 3];
        let part = dispatch(
            &mut s,
            "UPLOADPART",
            &[&upload_id, "1", std::str::from_utf8(&big).unwrap()],
        );
        assert!(matches!(part, S3Reply::OkHeaders(_)), "{part:?}");
        let snap = s
            .multipart
            .read()
            .unwrap()
            .uploads_snapshot(&upload_id)
            .unwrap();
        assert_eq!(snap.parts[0].size, big.len() as u64);
        let meta = crate::multipart::MultipartRegistry::encode_meta(&upload_id, &snap);
        assert!(meta.len() < 512, "registry must not embed the part body");
        let done = dispatch(&mut s, "COMPLETEMULTIPARTUPLOAD", &[&upload_id, ""]);
        assert!(matches!(done, S3Reply::Xml(_)), "{done:?}");
        match dispatch(&mut s, "GETOBJECT", &["bbb", "obj"]) {
            S3Reply::ChunkedObject {
                bucket,
                staging,
                total,
                chunk_count,
                ..
            } => {
                assert_eq!(total, big.len() as u64);
                let mut got = Vec::new();
                for index in 0..chunk_count {
                    got.extend(read_staged_chunk(&mut s, &bucket, &staging, index).expect("chunk"));
                }
                assert_eq!(got, big);
            }
            other => panic!("large complete must publish stored pieces: {other:?}"),
        }
    }

    #[test]
    fn get_and_head_fail_closed_on_read_quorum() {
        use spacestorage_query::options::Sourced;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(
            dispatch(&mut s, "PUTOBJECT", &["bbb", "k", "abc"]),
            S3Reply::OkHeaders(_)
        ));
        match dispatch(&mut s, "GETOBJECT", &["bbb", "k"]) {
            S3Reply::Body(b) => assert_eq!(b.bytes, b"abc"),
            other => panic!("{other:?}"),
        }
        match dispatch(&mut s, "HEADOBJECT", &["bbb", "k"]) {
            S3Reply::Head { content_length, .. } => assert_eq!(content_length, 3),
            other => panic!("{other:?}"),
        }
        s.query_options.quorum_read = Sourced::Session("TWO".into());
        for verb in ["GETOBJECT", "HEADOBJECT"] {
            match dispatch(&mut s, verb, &["bbb", "k"]) {
                S3Reply::Error { code, .. } => assert_eq!(code, "QuorumUnsatisfiable", "{verb}"),
                other => panic!("{verb} must not answer from local catalog: {other:?}"),
            }
        }
    }

    #[test]
    fn list_fails_closed_on_quorum_and_point_errors() {
        use spacestorage_query::options::Sourced;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(
            dispatch(&mut s, "PUTOBJECT", &["bbb", "k", "abc"]),
            S3Reply::OkHeaders(_)
        ));
        match dispatch(&mut s, "LISTOBJECTS", &["bbb", ""]) {
            S3Reply::Xml(x) => assert!(x.contains("<Size>3</Size>"), "{x}"),
            other => panic!("{other:?}"),
        }
        s.query_options.quorum_read = Sourced::Session("TWO".into());
        for verb in ["LISTOBJECTS", "LISTOBJECTSV2"] {
            match dispatch(&mut s, verb, &["bbb", ""]) {
                S3Reply::Error { code, .. } => assert_eq!(code, "QuorumUnsatisfiable", "{verb}"),
                other => panic!("{verb} must fail closed: {other:?}"),
            }
        }
        s.query_options.quorum_read = Sourced::BuiltIn("ONE".into());
        s.query_options.timeout = Sourced::Session(std::time::Duration::ZERO);
        match dispatch(&mut s, "LISTOBJECTSV2", &["bbb", ""]) {
            S3Reply::Error { .. } => {}
            S3Reply::Xml(x) => panic!("point error must not become a zero-size key: {x}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn object_key_exists_fails_closed() {
        use spacestorage_query::options::Sourced;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(
            dispatch(&mut s, "PUTOBJECT", &["bbb", "k", "keep"]),
            S3Reply::OkHeaders(_)
        ));
        s.query_options.timeout = Sourced::Session(std::time::Duration::ZERO);
        match s.object_key_exists("bbb", "k") {
            Err(S3Reply::Error { .. }) => {}
            other => panic!("exec error must not look like a missing key: {other:?}"),
        }
        s.query_options.timeout = Sourced::BuiltIn(std::time::Duration::from_secs(30));
        s.query_options.quorum_read = Sourced::Session("TWO".into());
        match s.object_key_exists("bbb", "absent") {
            Err(S3Reply::Error { code, .. }) => assert_eq!(code, "QuorumUnsatisfiable"),
            other => panic!("unmet read quorum must not report the key absent: {other:?}"),
        }
    }

    #[test]
    fn delete_object_does_not_ack_failed_exec() {
        use spacestorage_query::options::Sourced;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(
            dispatch(&mut s, "PUTOBJECT", &["bbb", "k", "keep"]),
            S3Reply::OkHeaders(_)
        ));
        s.query_options.timeout = Sourced::Session(std::time::Duration::ZERO);
        match dispatch(&mut s, "DELETEOBJECT", &["bbb", "k"]) {
            S3Reply::Error { .. } => {}
            S3Reply::NoContent => panic!("failed durable delete must not be acknowledged"),
            other => panic!("{other:?}"),
        }
        s.query_options.timeout = Sourced::BuiltIn(std::time::Duration::from_secs(30));
        match dispatch(&mut s, "GETOBJECT", &["bbb", "k"]) {
            S3Reply::Body(b) => assert_eq!(b.bytes, b"keep"),
            other => panic!("key must survive the failed delete: {other:?}"),
        }
        assert!(matches!(
            dispatch(&mut s, "DELETEOBJECT", &["bbb", "k"]),
            S3Reply::NoContent
        ));
        match dispatch(&mut s, "GETOBJECT", &["bbb", "k"]) {
            S3Reply::Error { code, .. } => assert_eq!(code, "NoSuchKey"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn delete_bucket_does_not_ack_failed_exec() {
        use spacestorage_query::options::Sourced;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(dispatch(&mut s, "CREATEBUCKET", &["bbb"]), S3Reply::Ok));
        s.query_options.timeout = Sourced::Session(std::time::Duration::ZERO);
        match dispatch(&mut s, "DELETEBUCKET", &["bbb"]) {
            S3Reply::Error { .. } => {}
            S3Reply::NoContent => panic!("failed durable drop must not be acknowledged"),
            other => panic!("{other:?}"),
        }
        assert!(s.bucket_exists("bbb"));
    }

    #[test]
    fn delete_objects_surfaces_failed_exec() {
        use spacestorage_query::options::Sourced;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(
            dispatch(&mut s, "PUTOBJECT", &["bbb", "k", "keep"]),
            S3Reply::OkHeaders(_)
        ));
        s.query_options.timeout = Sourced::Session(std::time::Duration::ZERO);
        let body = "<Delete><Object><Key>k</Key></Object></Delete>";
        match dispatch(&mut s, "DELETEOBJECTS", &["bbb", body]) {
            S3Reply::Xml(x) => {
                assert!(!x.contains("<Deleted>"), "failed key must not be listed deleted: {x}");
                assert!(x.contains("<Error>"), "{x}");
                assert!(x.contains("<Key>k</Key>"), "{x}");
            }
            other => panic!("{other:?}"),
        }
        s.query_options.timeout = Sourced::BuiltIn(std::time::Duration::from_secs(30));
        match dispatch(&mut s, "GETOBJECT", &["bbb", "k"]) {
            S3Reply::Body(b) => assert_eq!(b.bytes, b"keep"),
            other => panic!("key must survive failed batch delete: {other:?}"),
        }
    }

    #[test]
    fn abort_multipart_does_not_ack_failed_registry_delete() {
        use spacestorage_query::options::Sourced;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat)).with_multipart_limits(1, 100);
        assert!(matches!(dispatch(&mut s, "CREATEBUCKET", &["bbb"]), S3Reply::Ok));
        let init = dispatch(&mut s, "CREATEMULTIPARTUPLOAD", &["bbb", "obj"]);
        let S3Reply::Xml(xml) = init else {
            panic!("{init:?}");
        };
        let upload_id = xml
            .split("<UploadId>")
            .nth(1)
            .and_then(|r| r.split("</UploadId>").next())
            .unwrap()
            .to_string();
        s.query_options.timeout = Sourced::Session(std::time::Duration::ZERO);
        match dispatch(&mut s, "ABORTMULTIPARTUPLOAD", &[&upload_id]) {
            S3Reply::Error { .. } => {}
            S3Reply::NoContent => panic!("failed durable abort must not be acknowledged"),
            other => panic!("{other:?}"),
        }
        assert!(
            s.multipart
                .write()
                .unwrap()
                .get_for(&upload_id, &s.namespace)
                .is_some(),
            "in-memory upload must remain when durable abort failed"
        );
    }

    #[test]
    fn multipart_list_and_load_fail_closed() {
        use spacestorage_query::options::Sourced;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(Arc::clone(&cat)).with_multipart_limits(1, 100);
        assert!(matches!(dispatch(&mut s, "CREATEBUCKET", &["bbb"]), S3Reply::Ok));
        let init = dispatch(&mut s, "CREATEMULTIPARTUPLOAD", &["bbb", "obj"]);
        let S3Reply::Xml(xml) = init else {
            panic!("{init:?}");
        };
        let upload_id = xml
            .split("<UploadId>")
            .nth(1)
            .and_then(|r| r.split("</UploadId>").next())
            .unwrap()
            .to_string();
        s.multipart.write().unwrap().remove_for(&upload_id, &s.namespace);
        s.query_options.quorum_read = Sourced::Session("TWO".into());
        match dispatch(&mut s, "LISTPARTS", &["bbb", "obj", &upload_id]) {
            S3Reply::Error { code, .. } => assert_eq!(code, "QuorumUnsatisfiable"),
            other => panic!("LISTPARTS must fail closed: {other:?}"),
        }
        s.query_options.quorum_read = Sourced::BuiltIn("ONE".into());
        s.query_options.timeout = Sourced::Session(std::time::Duration::ZERO);
        match dispatch(&mut s, "LISTPARTS", &["bbb", "obj", &upload_id]) {
            S3Reply::Error { code, .. } if code == "NoSuchUpload" => {
                panic!("point error must not masquerade as NoSuchUpload")
            }
            S3Reply::Error { .. } => {}
            other => panic!("LISTPARTS must fail closed: {other:?}"),
        }
        s.query_options.timeout = Sourced::BuiltIn(std::time::Duration::from_secs(30));
        match dispatch(&mut s, "LISTMULTIPARTUPLOADS", &["bbb"]) {
            S3Reply::Xml(x) if x.contains(&upload_id) => {}
            S3Reply::Xml(x) => panic!("durable upload must be listed after reload: {x}"),
            other => panic!("{other:?}"),
        }
    }
}
