//! HTTP/1.1 path-style S3 wire (`/{bucket}/{key}`) with SigV4 auth.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use bytes::BytesMut;
use spacestorage_compat::{DialectProfile, LimitServices};
use spacestorage_protocol_core::{
    ProtocolFamily, default_timeout, parse_http_request_line, peek_signature, split_target,
};
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::auth::{CredentialStore, check_session_token};
use crate::dispatch::{
    ObjectReply, S3Reply, SessionState, abort_staged_chunks, append_upload_part_window,
    apply_query_headers, commit_staged_object, copy_object, dispatch, finish_upload_part,
    open_upload_part, put_object_with_meta, stage_object_chunk, upload_part_bytes,
};
use crate::meta::ObjectMeta;
use crate::router::{route_http, route_subresource};
use crate::sigv4::{
    SigV4Error, SigV4Request, now_unix, parse_authorization, payload_hash_from_headers,
    request_time_skewed, sha256_hex_bytes, verify_presigned, verify_sigv4_with_secret,
};
use crate::stream::decode_aws_chunked;

#[derive(Clone)]
pub struct S3Handler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub profile: DialectProfile,
    pub namespace: String,
    pub limit_services: LimitServices,
    pub tenancy: Option<Arc<spacestorage_tenancy::Tenancy>>,
    pub region: String,
    pub virtual_host_suffix: Option<String>,
    pub multipart_idle: std::time::Duration,
    pub multipart_min_part: u64,
    pub multipart_max_parts: u32,
    pub credentials: Arc<CredentialStore>,
    pub multipart_registry: Arc<RwLock<crate::multipart::MultipartRegistry>>,
}

impl S3Handler {
    pub fn with_demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self::with_limits(catalog, LimitServices::built_in())
    }

    pub fn with_limits(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
    ) -> Self {
        Self {
            catalog,
            profile: DialectProfile::HandlersComplete,
            namespace: "demo".into(),
            limit_services,
            tenancy: None,
            region: "us-east-1".into(),
            virtual_host_suffix: None,
            multipart_idle: std::time::Duration::from_secs(24 * 3600),
            multipart_min_part: 5 * 1024 * 1024,
            multipart_max_parts: 10_000,
            credentials: CredentialStore::with_demo_default(),
            multipart_registry: Arc::new(RwLock::new(
                crate::multipart::MultipartRegistry::default(),
            )),
        }
    }

    pub fn with_protocol_config(
        mut self,
        region: String,
        virtual_host_suffix: Option<String>,
        multipart_idle: std::time::Duration,
        multipart_min_part: u64,
        multipart_max_parts: u32,
    ) -> Self {
        self.region = region;
        self.virtual_host_suffix = virtual_host_suffix;
        self.multipart_idle = multipart_idle;
        self.multipart_min_part = multipart_min_part;
        self.multipart_max_parts = multipart_max_parts;
        self.multipart_registry = Arc::new(RwLock::new(crate::multipart::MultipartRegistry::new(
            multipart_idle,
            multipart_min_part,
            multipart_max_parts,
        )));
        self
    }

    pub fn with_tenancy(mut self, tenancy: Arc<spacestorage_tenancy::Tenancy>) -> Self {
        self.tenancy = Some(tenancy);
        self
    }

    /// Raise `max_value` / `max_key` for Suite B and large-object fixtures.
    pub fn with_value_limits(mut self, max_value: u64, max_key: u64) -> Self {
        let mut limits = self.limit_services.limits();
        limits.max_value = max_value;
        limits.max_key = max_key;
        self.limit_services =
            LimitServices::with_fixed(Arc::clone(self.limit_services.counters()), limits);
        self
    }

    pub async fn serve<S>(&self, mut stream: S, cancel: CancellationToken)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let peeked = match peek_signature(&mut stream, ProtocolFamily::S3, default_timeout()).await
        {
            Ok(buf) => buf,
            Err((_err, refusal)) => {
                let _ = stream.write_all(&refusal.body).await;
                let _ = stream.shutdown().await;
                return;
            }
        };

        let mut session = SessionState::demo(Arc::clone(&self.catalog))
            .with_multipart_limits(self.multipart_min_part, self.multipart_max_parts);
        session.profile = self.profile;
        session.namespace = self.namespace.clone();
        session.limit_services = self.limit_services.clone();
        session.tenancy = self.tenancy.clone();
        session.region = self.region.clone();
        session.multipart = Arc::clone(&self.multipart_registry);
        session.multipart_min_part = self.multipart_min_part;
        session.multipart_max_parts = self.multipart_max_parts;
        if let Ok(mut reg) = self.multipart_registry.write() {
            reg.set_idle(self.multipart_idle);
        }

        let mut buf = BytesMut::from(peeked.as_slice());

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                req = read_http_headers(&mut stream, &mut buf) => {
                    match req {
                        Ok(None) => break,
                        Ok(Some(mut parsed)) => {
                            let request_id = next_request_id();
                            if std::env::var_os("S3_TRACE").is_some() {
                                eprintln!(
                                    "s3 {request_id} {} {} q={}",
                                    parsed.method,
                                    parsed.path,
                                    parsed.query.as_deref().unwrap_or("")
                                );
                            }
                            if parsed.expect_continue {
                                if stream
                                    .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                                let _ = stream.flush().await;
                            }
                            let resource = parsed.path.clone();
                            let max_value = session.limit_services.limits().max_value;
                            let reply = if should_stream_put(&parsed) {
                                if !parsed.chunked && (parsed.content_length as u64) > max_value {
                                    parsed.keep_alive = false;
                                    value_limit_reply(parsed.content_length as u64, max_value)
                                } else if is_upload_part(&parsed) {
                                    stream_upload_part(
                                        &mut session,
                                        &mut stream,
                                        &mut buf,
                                        &parsed,
                                        &self.credentials,
                                        self.virtual_host_suffix.as_deref(),
                                    )
                                    .await
                                } else {
                                    stream_put_object(
                                        &mut session,
                                        &mut stream,
                                        &mut buf,
                                        &parsed,
                                        &self.credentials,
                                        self.virtual_host_suffix.as_deref(),
                                    )
                                    .await
                                }
                            } else if body_exceeds_value(&parsed, max_value) {
                                parsed.keep_alive = false;
                                value_limit_reply(parsed.content_length as u64, max_value)
                            } else {
                                match read_request_body(&mut stream, &mut buf, &parsed).await {
                                    Ok(body) => {
                                        parsed.body = body;
                                        handle_http(
                                            &mut session,
                                            &parsed,
                                            &self.credentials,
                                            self.virtual_host_suffix.as_deref(),
                                        )
                                    }
                                    Err(_) => break,
                                }
                            };
                            if write_http_response(
                                &mut stream,
                                &mut session,
                                &reply,
                                &request_id,
                                parsed.keep_alive,
                                &resource,
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                            if !parsed.keep_alive {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct ParsedHttpRequest {
    method: String,
    path: String,
    query: Option<String>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    keep_alive: bool,
    content_length: usize,
    chunked: bool,
    expect_continue: bool,
}

impl ParsedHttpRequest {
    fn with_body(&self, body: Vec<u8>) -> Self {
        Self {
            method: self.method.clone(),
            path: self.path.clone(),
            query: self.query.clone(),
            headers: self.headers.clone(),
            body,
            keep_alive: self.keep_alive,
            content_length: self.content_length,
            chunked: self.chunked,
            expect_continue: self.expect_continue,
        }
    }
}

fn io_to_s3(err: std::io::Error) -> S3Reply {
    S3Reply::Error {
        code: "InternalError".into(),
        message: err.to_string(),
    }
}

fn next_request_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    format!("{:016x}", SEQ.fetch_add(1, Ordering::Relaxed))
}

fn x_amz_id_2(request_id: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(request_id.as_bytes()))
}

async fn read_http_headers<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
) -> std::io::Result<Option<ParsedHttpRequest>> {
    loop {
        if let Some(req) = try_parse_headers(buf)? {
            return Ok(Some(req));
        }
        if !read_some(stream, buf).await? {
            return Ok(None);
        }
    }
}

fn try_parse_headers(buf: &mut BytesMut) -> std::io::Result<Option<ParsedHttpRequest>> {
    let Some(header_end) = find_header_end(buf) else {
        return Ok(None);
    };
    let header_bytes = buf[..header_end].to_vec();
    let headers_str = std::str::from_utf8(&header_bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut lines = headers_str.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let Some(line) = parse_http_request_line(request_line) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad request line",
        ));
    };
    let (path, query) = split_target(&line.target);
    let mut headers = Vec::new();
    for h in lines {
        if h.is_empty() {
            continue;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let content_length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let chunked = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")
    });
    let expect_continue = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("expect") && v.to_ascii_lowercase().contains("100-continue")
    });
    let keep_alive = match headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("connection"))
        .map(|(_, v)| v.as_str())
    {
        Some(v) if v.eq_ignore_ascii_case("close") => false,
        Some(v) if v.eq_ignore_ascii_case("keep-alive") => true,
        _ => line.version == "HTTP/1.1",
    };
    buf.advance(header_end + 4);
    Ok(Some(ParsedHttpRequest {
        method: line.method,
        path: path.to_string(),
        query: query.map(str::to_string),
        headers,
        body: Vec::new(),
        keep_alive,
        content_length,
        chunked,
        expect_continue,
    }))
}

async fn read_some<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
) -> std::io::Result<bool> {
    let mut tmp = [0u8; 8192];
    let n = stream.read(&mut tmp).await?;
    if n == 0 {
        return Ok(false);
    }
    buf.extend_from_slice(&tmp[..n]);
    Ok(true)
}

async fn read_exact_buf<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
    dest: &mut [u8],
) -> std::io::Result<()> {
    let mut filled = 0;
    while filled < dest.len() {
        if buf.is_empty() && !read_some(stream, buf).await? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "short body",
            ));
        }
        let n = (dest.len() - filled).min(buf.len());
        dest[filled..filled + n].copy_from_slice(&buf[..n]);
        let _ = buf.split_to(n);
        filled += n;
    }
    Ok(())
}

async fn read_crlf_line_limited<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
    remain: &mut usize,
) -> Result<String, S3Reply> {
    loop {
        let window = buf.len().min(*remain);
        if window >= 2 {
            if let Some(pos) = buf[..window].windows(2).position(|w| w == b"\r\n") {
                let line = String::from_utf8_lossy(&buf[..pos]).into_owned();
                let _ = buf.split_to(pos + 2);
                *remain -= pos + 2;
                return Ok(line);
            }
        }
        if buf.len() >= *remain {
            return Err(S3Reply::Error {
                code: "InvalidArgument".into(),
                message: "truncated chunk header".into(),
            });
        }
        if !read_some(stream, buf).await.map_err(io_to_s3)? {
            return Err(S3Reply::Error {
                code: "InvalidArgument".into(),
                message: "truncated chunk header".into(),
            });
        }
    }
}

async fn read_exact_limited<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
    dest: &mut [u8],
    remain: &mut usize,
) -> Result<(), S3Reply> {
    if dest.len() > *remain {
        return Err(S3Reply::Error {
            code: "InvalidArgument".into(),
            message: "truncated chunk".into(),
        });
    }
    read_exact_buf(stream, buf, dest).await.map_err(io_to_s3)?;
    *remain -= dest.len();
    Ok(())
}

fn parse_aws_chunk_header(line: &str) -> (&str, String) {
    match line.split_once(';') {
        Some((size, rest)) => {
            let sig = rest
                .split(';')
                .find_map(|p| p.trim().strip_prefix("chunk-signature="))
                .unwrap_or("")
                .to_string();
            (size.trim(), sig)
        }
        None => (line.trim(), String::new()),
    }
}

struct ChunkSigner {
    secret: String,
    date: String,
    region: String,
    service: String,
    amz_date: String,
    scope: String,
    previous: String,
}

fn prepare_chunk_signer(
    req: &ParsedHttpRequest,
    credentials: &CredentialStore,
) -> Result<ChunkSigner, S3Reply> {
    let authorization = header_lookup(&req.headers, "authorization").unwrap_or("");
    let parsed = parse_authorization(authorization).map_err(|_| S3Reply::Error {
        code: "SignatureDoesNotMatch".into(),
        message: "bad authorization".into(),
    })?;
    let cred = credentials
        .lookup(&parsed.access_key)
        .ok_or_else(|| S3Reply::Error {
            code: "InvalidAccessKeyId".into(),
            message: "Unknown access key".into(),
        })?;
    let amz_date = header_lookup(&req.headers, "x-amz-date")
        .unwrap_or("")
        .to_string();
    let scope = format!(
        "{}/{}/{}/aws4_request",
        parsed.date, parsed.region, parsed.service
    );
    Ok(ChunkSigner {
        secret: cred.secret.clone(),
        date: parsed.date,
        region: parsed.region,
        service: parsed.service,
        amz_date,
        scope,
        previous: parsed.signature,
    })
}

fn verify_chunk_sig(
    signer: &mut ChunkSigner,
    presented: &str,
    chunk_sha: String,
) -> Result<(), S3Reply> {
    if presented.is_empty() {
        return Err(S3Reply::Error {
            code: "SignatureDoesNotMatch".into(),
            message: "chunk signature mismatch".into(),
        });
    }
    let empty = crate::sigv4::sha256_hex_bytes(b"");
    let sts = format!(
        "AWS4-HMAC-SHA256-PAYLOAD\n{}\n{}\n{}\n{empty}\n{chunk_sha}",
        signer.amz_date, signer.scope, signer.previous
    );
    let expect = crate::sigv4::sign_chunk(
        &signer.secret,
        &signer.date,
        &signer.region,
        &signer.service,
        &sts,
    );
    if expect != presented {
        return Err(S3Reply::Error {
            code: "SignatureDoesNotMatch".into(),
            message: "chunk signature mismatch".into(),
        });
    }
    signer.previous = expect;
    Ok(())
}

async fn read_crlf_line<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
) -> std::io::Result<String> {
    loop {
        if let Some(pos) = buf.windows(2).position(|w| w == b"\r\n") {
            let line = String::from_utf8_lossy(&buf[..pos]).into_owned();
            let _ = buf.split_to(pos + 2);
            return Ok(line);
        }
        if !read_some(stream, buf).await? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "short chunk header",
            ));
        }
    }
}

async fn read_request_body<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
    req: &ParsedHttpRequest,
) -> std::io::Result<Vec<u8>> {
    if req.chunked {
        return read_chunked_body(stream, buf, usize::MAX).await;
    }
    let mut body = vec![0u8; req.content_length];
    if !body.is_empty() {
        read_exact_buf(stream, buf, &mut body).await?;
    }
    Ok(body)
}

async fn read_chunked_body<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line = read_crlf_line(stream, buf).await?;
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        if size == 0 {
            loop {
                let trailer = read_crlf_line(stream, buf).await?;
                if trailer.is_empty() {
                    break;
                }
            }
            break;
        }
        if out.len().saturating_add(size) > limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "chunked body over limit",
            ));
        }
        let mut piece = vec![0u8; size];
        read_exact_buf(stream, buf, &mut piece).await?;
        let mut crlf = [0u8; 2];
        read_exact_buf(stream, buf, &mut crlf).await?;
        out.extend_from_slice(&piece);
    }
    Ok(out)
}

fn body_exceeds_value(req: &ParsedHttpRequest, max_value: u64) -> bool {
    if req.chunked {
        return false;
    }
    let put = req.method.eq_ignore_ascii_case("PUT");
    put && (req.content_length as u64) > max_value
}

fn value_limit_reply(current: u64, max: u64) -> S3Reply {
    S3Reply::Error {
        code: "limit_exceeded".into(),
        message: format!("limit_exceeded{{limit:value,current:{current},max:{max}}}"),
    }
}

fn is_aws_streaming_payload(mode: &str) -> bool {
    mode == "STREAMING-AWS4-HMAC-SHA256-PAYLOAD" || mode == "STREAMING-UNSIGNED-PAYLOAD-TRAILER"
}

fn should_stream_put(req: &ParsedHttpRequest) -> bool {
    if !req.method.eq_ignore_ascii_case("PUT") {
        return false;
    }
    if header_lookup(&req.headers, "x-amz-copy-source").is_some() {
        return false;
    }
    let query = req.query.as_deref().map(parse_query).unwrap_or_default();
    if query.contains_key("uploadId") && query.contains_key("partNumber") {
        return true;
    }
    let (_, key) = split_bucket_key(&req.path);
    if key.is_none() {
        return false;
    }
    let mode = header_lookup(&req.headers, "x-amz-content-sha256").unwrap_or("");
    if is_aws_streaming_payload(mode) {
        return true;
    }
    req.chunked || req.content_length > crate::stream::CHUNK_WINDOW
}

fn is_upload_part(req: &ParsedHttpRequest) -> bool {
    let query = req.query.as_deref().map(parse_query).unwrap_or_default();
    query.contains_key("uploadId") && query.contains_key("partNumber")
}

fn reject_if_none_match_star(
    session: &SessionState,
    bucket: &str,
    key: &str,
    headers: &[(String, String)],
) -> Result<(), S3Reply> {
    if header_lookup(headers, "if-none-match").is_some_and(|m| m.trim() == "*") {
        match session.object_key_exists(bucket, key) {
            Ok(true) => {
                return Err(S3Reply::Error {
                    code: "PreconditionFailed".into(),
                    message: "If-None-Match: *".into(),
                });
            }
            Ok(false) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn observe_s3(kind: &str) {
    tracing::info!(protocol = "s3", kind, "s3 request");
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

trait BytesAdvance {
    fn advance(&mut self, n: usize);
}

impl BytesAdvance for BytesMut {
    fn advance(&mut self, n: usize) {
        let _ = self.split_to(n);
    }
}

fn header_lookup<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn header_pairs(headers: &[(String, String)]) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

struct AuthOk {
    access_key: String,
    namespace_bound: bool,
    namespace: Option<String>,
    grants: u16,
    login: String,
}

fn auth_from_cred(
    access_key: String,
    cred: &crate::auth::S3Credential,
    force_unbound: bool,
) -> AuthOk {
    AuthOk {
        access_key,
        namespace_bound: cred.namespace.is_some() && !force_unbound,
        namespace: if force_unbound {
            None
        } else {
            cred.namespace.clone()
        },
        grants: cred.grants,
        login: cred.login.clone(),
    }
}

/// `x-spacestorage-demo-auth` resolves the key in the 014 store (Suite A only — FR-002).
fn authenticate(
    req: &ParsedHttpRequest,
    credentials: &CredentialStore,
) -> Result<AuthOk, (String, String)> {
    let force_unbound = header_lookup(&req.headers, "x-spacestorage-unbound-test") == Some("1");
    let query = req.query.as_deref().map(parse_query).unwrap_or_default();
    let session_tok = header_lookup(&req.headers, "x-amz-security-token")
        .or_else(|| query_get(&query, "X-Amz-Security-Token"));

    if header_lookup(&req.headers, "x-spacestorage-demo-auth").is_some() {
        #[cfg(not(any(test, feature = "suite-a-demo-auth")))]
        {
            return Err((
                "AccessDenied".into(),
                "demo auth is not enabled in this build".into(),
            ));
        }
        #[cfg(any(test, feature = "suite-a-demo-auth"))]
        {
            let key_id = header_lookup(&req.headers, "x-spacestorage-demo-auth").unwrap();
            let Some(cred) = credentials.lookup(key_id) else {
                return Err(("InvalidAccessKeyId".into(), "Unknown access key".into()));
            };
            if let Err((code, msg)) = check_session_token(&cred, session_tok) {
                return Err((code.into(), msg.into()));
            }
            return Ok(auth_from_cred(key_id.into(), &cred, force_unbound));
        }
    }

    if query_get(&query, "X-Amz-Algorithm").is_some() {
        let raw_query = req.query.as_deref().unwrap_or("");
        let host = header_lookup(&req.headers, "host").unwrap_or("localhost");
        let header_pairs = [("host", host)];
        let access_guess = query_get(&query, "X-Amz-Credential")
            .unwrap_or("")
            .split(|c: char| c == '/' || c == '%')
            .next()
            .unwrap_or("");
        let Some(cred) = credentials.lookup(access_guess) else {
            return Err(("InvalidAccessKeyId".into(), "Unknown access key".into()));
        };
        match verify_presigned(
            &req.method,
            &req.path,
            raw_query,
            &header_pairs,
            &cred.secret,
            now_unix(),
        ) {
            Ok(ok) => {
                let presented = ok.session_token.as_deref().or(session_tok);
                if let Err((code, msg)) = check_session_token(&cred, presented) {
                    return Err((code.into(), msg.into()));
                }
                return Ok(auth_from_cred(ok.access_key, &cred, force_unbound));
            }
            Err(SigV4Error::Expired) => {
                return Err(("AccessDenied".into(), "Request has expired".into()));
            }
            Err(SigV4Error::UnknownAccessKey) => {
                return Err(("InvalidAccessKeyId".into(), "Unknown access key".into()));
            }
            Err(_) => {
                return Err((
                    "SignatureDoesNotMatch".into(),
                    "The request signature does not match".into(),
                ));
            }
        }
    }

    let authorization = header_lookup(&req.headers, "authorization");
    if authorization.is_none() {
        return Err(("AccessDenied".into(), "Anonymous denied".into()));
    }
    let authorization = authorization.unwrap();
    if authorization.starts_with("AWS ") {
        return Err(("InvalidArgument".into(), "SigV2 not supported".into()));
    }
    if !authorization.starts_with("AWS4-HMAC-SHA256") {
        return Err(("AccessDenied".into(), "Anonymous denied".into()));
    }

    let amz_date = header_lookup(&req.headers, "x-amz-date")
        .ok_or(("AccessDenied".into(), "Missing x-amz-date".into()))?;
    let payload_hash = payload_hash_from_headers(&header_pairs(&req.headers))
        .map_err(|_| ("AccessDenied".into(), "Missing x-amz-content-sha256".into()))?;
    let sig_req = SigV4Request {
        method: &req.method,
        uri_path: &req.path,
        query: req.query.as_deref(),
        headers: &header_pairs(&req.headers),
        payload_hash: &payload_hash,
    };
    let parsed = parse_authorization(authorization).map_err(|_| {
        (
            "SignatureDoesNotMatch".into(),
            "The request signature does not match".into(),
        )
    })?;
    let Some(cred) = credentials.lookup(&parsed.access_key) else {
        return Err(("InvalidAccessKeyId".into(), "Unknown access key".into()));
    };
    if let Err(e) = verify_sigv4_with_secret(&sig_req, authorization, amz_date, &cred.secret) {
        let (code, msg) = match e {
            SigV4Error::BadSignature | SigV4Error::BadAuthorization => (
                "SignatureDoesNotMatch".into(),
                "The request signature does not match".into(),
            ),
            SigV4Error::UnknownAccessKey => {
                ("InvalidAccessKeyId".into(), "Unknown access key".into())
            }
            _ => ("AccessDenied".into(), "Auth failed".into()),
        };
        return Err((code, msg));
    }
    if request_time_skewed(amz_date, now_unix()) {
        return Err((
            "RequestTimeTooSkewed".into(),
            "The difference between the request time and the current time is too large.".into(),
        ));
    }
    if let Err((code, msg)) = check_session_token(&cred, session_tok) {
        return Err((code.into(), msg.into()));
    }
    Ok(auth_from_cred(parsed.access_key, &cred, force_unbound))
}

fn query_get<'a>(query: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            Some((
                crate::router::percent_decode(k),
                crate::router::percent_decode(v),
            ))
        })
        .collect()
}

fn split_bucket_key(path: &str) -> (Option<String>, Option<String>) {
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return (None, None);
    }
    match path.split_once('/') {
        None => (Some(crate::router::percent_decode(path)), None),
        Some((bucket, rest)) if rest.is_empty() => {
            (Some(crate::router::percent_decode(bucket)), None)
        }
        Some((bucket, key)) => (
            Some(crate::router::percent_decode(bucket)),
            Some(crate::router::percent_decode(key)),
        ),
    }
}

async fn pump_request_body<S, F>(
    stream: &mut S,
    buf: &mut BytesMut,
    req: &ParsedHttpRequest,
    credentials: &CredentialStore,
    mut on_piece: F,
) -> Result<(), S3Reply>
where
    S: AsyncRead + Unpin,
    F: FnMut(Vec<u8>) -> Result<(), S3Reply>,
{
    use md5::Digest as _;
    let payload_mode = header_lookup(&req.headers, "x-amz-content-sha256").unwrap_or("");
    let aws_framed = is_aws_streaming_payload(payload_mode);
    let mut chunk_sig = if payload_mode == "STREAMING-AWS4-HMAC-SHA256-PAYLOAD" {
        match prepare_chunk_signer(req, credentials) {
            Ok(sig) => Some(sig),
            Err(reply) => return Err(reply),
        }
    } else {
        None
    };

    if aws_framed {
        let mut remain = req.content_length;
        loop {
            let line = read_crlf_line_limited(stream, buf, &mut remain).await?;
            let (size_hex, sig) = parse_aws_chunk_header(&line);
            let size = usize::from_str_radix(size_hex, 16).map_err(|e| S3Reply::Error {
                code: "InvalidArgument".into(),
                message: e.to_string(),
            })?;
            if size == 0 {
                if let Some(signer) = chunk_sig.as_mut() {
                    verify_chunk_sig(signer, &sig, crate::sigv4::sha256_hex_bytes(b""))?;
                }
                while remain > 0 {
                    let trailer = read_crlf_line_limited(stream, buf, &mut remain).await?;
                    if trailer.is_empty() {
                        break;
                    }
                }
                break;
            }
            let mut left = size;
            let mut hasher = sha2::Sha256::new();
            while left > 0 {
                let n = left.min(crate::stream::CHUNK_WINDOW);
                let mut piece = vec![0u8; n];
                read_exact_limited(stream, buf, &mut piece, &mut remain).await?;
                sha2::Digest::update(&mut hasher, &piece);
                on_piece(piece)?;
                left -= n;
            }
            let mut crlf = [0u8; 2];
            read_exact_limited(stream, buf, &mut crlf, &mut remain).await?;
            if &crlf != b"\r\n" {
                return Err(S3Reply::Error {
                    code: "InvalidArgument".into(),
                    message: "missing chunk crlf".into(),
                });
            }
            if let Some(signer) = chunk_sig.as_mut() {
                verify_chunk_sig(signer, &sig, hex::encode(sha2::Digest::finalize(hasher)))?;
            }
        }
    } else if req.chunked {
        loop {
            let line = read_crlf_line(stream, buf).await.map_err(io_to_s3)?;
            let size_hex = line.split(';').next().unwrap_or("").trim();
            let size = usize::from_str_radix(size_hex, 16).map_err(|e| S3Reply::Error {
                code: "InvalidArgument".into(),
                message: e.to_string(),
            })?;
            if size == 0 {
                loop {
                    let trailer = read_crlf_line(stream, buf).await.map_err(io_to_s3)?;
                    if trailer.is_empty() {
                        break;
                    }
                }
                break;
            }
            let mut left = size;
            while left > 0 {
                let n = left.min(crate::stream::CHUNK_WINDOW);
                let mut piece = vec![0u8; n];
                read_exact_buf(stream, buf, &mut piece)
                    .await
                    .map_err(io_to_s3)?;
                on_piece(piece)?;
                left -= n;
            }
            let mut crlf = [0u8; 2];
            read_exact_buf(stream, buf, &mut crlf)
                .await
                .map_err(io_to_s3)?;
        }
    } else {
        let mut left = req.content_length;
        while left > 0 {
            let n = left.min(crate::stream::CHUNK_WINDOW);
            let mut piece = vec![0u8; n];
            read_exact_buf(stream, buf, &mut piece)
                .await
                .map_err(io_to_s3)?;
            on_piece(piece)?;
            left -= n;
        }
    }
    Ok(())
}

async fn stream_upload_part<S: AsyncRead + Unpin>(
    session: &mut SessionState,
    stream: &mut S,
    buf: &mut BytesMut,
    req: &ParsedHttpRequest,
    credentials: &CredentialStore,
    vhost_suffix: Option<&str>,
) -> S3Reply {
    if let Err(e) = bind_auth(session, req, credentials) {
        return e;
    }
    let host = header_lookup(&req.headers, "host").unwrap_or("localhost");
    if let Err(reply) = rewrite_vhost(host, &req.path, vhost_suffix) {
        return reply;
    }
    let query = req.query.as_deref().map(parse_query).unwrap_or_default();
    let Some(upload_id) = query.get("uploadId").map(String::as_str) else {
        return S3Reply::Error {
            code: "InvalidArgument".into(),
            message: "missing uploadId".into(),
        };
    };
    let part_number = query
        .get("partNumber")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1);
    observe_s3("UPLOADPART");
    let opened = match open_upload_part(session, upload_id, part_number) {
        Ok(part) => part,
        Err(e) => return e,
    };

    use md5::Digest as _;
    let max_value = session.limit_services.limits().max_value;
    let mut md5 = md5::Md5::new();
    let mut sha = sha2::Sha256::new();
    let mut crc32 = crc32fast::Hasher::new();
    let mut crc32c = crate::checksum::Crc32c::new();
    let mut held: Vec<u8> = Vec::new();
    let mut index = 0u32;
    let mut total = 0u64;
    let payload_mode = header_lookup(&req.headers, "x-amz-content-sha256")
        .unwrap_or("")
        .to_string();

    if let Err(reply) = pump_request_body(stream, buf, req, credentials, |piece| {
        total += piece.len() as u64;
        if total > max_value {
            return Err(value_limit_reply(total, max_value));
        }
        md5::Digest::update(&mut md5, &piece);
        sha2::Digest::update(&mut sha, &piece);
        crc32.update(&piece);
        crc32c.update(&piece);
        held.extend_from_slice(&piece);
        while held.len() >= crate::stream::CHUNK_WINDOW {
            let rest = held.split_off(crate::stream::CHUNK_WINDOW);
            let window = std::mem::replace(&mut held, rest);
            append_upload_part_window(session, &opened, index, window)?;
            index += 1;
        }
        Ok(())
    })
    .await
    {
        return reply;
    }
    if !held.is_empty() {
        if let Err(e) = append_upload_part_window(session, &opened, index, held) {
            return e;
        }
    }
    let md5_raw = md5::Digest::finalize(md5);
    let sha_raw = sha2::Digest::finalize(sha);
    if payload_mode.len() == 64 && payload_mode.chars().all(|c| c.is_ascii_hexdigit()) {
        let got = hex::encode(sha_raw.as_slice());
        if !got.eq_ignore_ascii_case(&payload_mode) {
            return S3Reply::Error {
                code: "XAmzContentSHA256Mismatch".into(),
                message: "payload hash mismatch".into(),
            };
        }
    }
    if let Err(e) = crate::checksum::check_stream_digests(
        &req.headers,
        &md5_raw,
        &sha_raw,
        crc32.finalize(),
        crc32c.finish(),
    ) {
        return e;
    }
    let etag = format!("\"{}\"", hex::encode(md5_raw.as_slice()));
    finish_upload_part(session, opened, etag, total)
}

async fn stream_put_object<S: AsyncRead + Unpin>(
    session: &mut SessionState,
    stream: &mut S,
    buf: &mut BytesMut,
    req: &ParsedHttpRequest,
    credentials: &CredentialStore,
    vhost_suffix: Option<&str>,
) -> S3Reply {
    if let Err(e) = bind_auth(session, req, credentials) {
        return e;
    }
    let host = header_lookup(&req.headers, "host").unwrap_or("localhost");
    let path = match rewrite_vhost(host, &req.path, vhost_suffix) {
        Ok(p) => p,
        Err(reply) => return reply,
    };
    let (bucket, key) = split_bucket_key(&path);
    let Some(bucket) = bucket else {
        return S3Reply::Error {
            code: "InvalidBucketName".into(),
            message: "missing bucket".into(),
        };
    };
    let Some(key) = key else {
        return S3Reply::Error {
            code: "InvalidArgument".into(),
            message: "missing key".into(),
        };
    };
    observe_s3("PUTOBJECT");
    if let Err(e) = reject_if_none_match_star(session, &bucket, &key, &req.headers) {
        return e;
    }
    use md5::Digest as _;
    let max_value = session.limit_services.limits().max_value;
    let staging = next_request_id();
    let mut index = 0u32;
    let mut total = 0u64;
    let mut staged = false;
    let mut held = Vec::new();
    let mut md5 = md5::Md5::new();
    let mut sha = sha2::Sha256::new();
    let mut crc32 = crc32fast::Hasher::new();
    let mut crc32c = crate::checksum::Crc32c::new();
    let mut payload_mode = header_lookup(&req.headers, "x-amz-content-sha256")
        .unwrap_or("")
        .to_string();
    let aws_framed = is_aws_streaming_payload(&payload_mode);
    if let Err(reply) = pump_request_body(stream, buf, req, credentials, |piece| {
        accept_piece(
            session,
            &bucket,
            &staging,
            &mut index,
            &mut total,
            &mut staged,
            &mut held,
            &mut md5,
            &mut sha,
            &mut crc32,
            &mut crc32c,
            max_value,
            piece,
        )
    })
    .await
    {
        if staged {
            abort_staged_chunks(session, &bucket, &staging, index);
        }
        return reply;
    }
    let md5_raw = md5.finalize();
    let sha_raw = sha.finalize();
    if payload_mode.len() == 64 && payload_mode.chars().all(|c| c.is_ascii_hexdigit()) {
        let got = hex::encode(sha_raw.as_slice());
        if !got.eq_ignore_ascii_case(&payload_mode) {
            if staged {
                abort_staged_chunks(session, &bucket, &staging, index);
            }
            return S3Reply::Error {
                code: "XAmzContentSHA256Mismatch".into(),
                message: "payload hash mismatch".into(),
            };
        }
    }
    payload_mode.clear();
    if let Err(e) = crate::checksum::check_stream_digests(
        &req.headers,
        &md5_raw,
        &sha_raw,
        crc32.finalize(),
        crc32c.finish(),
    ) {
        if staged {
            abort_staged_chunks(session, &bucket, &staging, index);
        }
        return e;
    }
    if !staged {
        if aws_framed {
            let mut meta = meta_from_headers(&req.headers);
            meta.etag = format!("\"{}\"", hex::encode(md5_raw.as_slice()));
            return put_object_with_meta(session, &bucket, &key, held, meta);
        }
        let inline = req.with_body(held);
        return handle_http(session, &inline, credentials, vhost_suffix);
    }
    let mut meta = meta_from_headers(&req.headers);
    meta.etag = format!("\"{}\"", hex::encode(md5_raw.as_slice()));
    commit_staged_object(
        session,
        &bucket,
        &key,
        meta,
        &staging,
        total,
        crate::stream::CHUNK_WINDOW,
        index,
    )
}

fn accept_piece(
    session: &mut SessionState,
    bucket: &str,
    staging: &str,
    index: &mut u32,
    total: &mut u64,
    staged: &mut bool,
    held: &mut Vec<u8>,
    md5: &mut md5::Md5,
    sha: &mut sha2::Sha256,
    crc32: &mut crc32fast::Hasher,
    crc32c: &mut crate::checksum::Crc32c,
    max_value: u64,
    piece: Vec<u8>,
) -> Result<(), S3Reply> {
    use md5::Digest as _;
    *total += piece.len() as u64;
    if *total > max_value {
        return Err(value_limit_reply(*total, max_value));
    }
    md5.update(&piece);
    sha.update(&piece);
    crc32.update(&piece);
    crc32c.update(&piece);
    if !*staged && held.len() + piece.len() <= crate::stream::CHUNK_WINDOW {
        held.extend_from_slice(&piece);
        return Ok(());
    }
    if !*staged {
        let mut off = 0;
        while off < held.len() {
            let end = (off + crate::stream::CHUNK_WINDOW).min(held.len());
            stage_object_chunk(session, bucket, staging, *index, held[off..end].to_vec())?;
            *index += 1;
            off = end;
        }
        held.clear();
        *staged = true;
    }
    stage_object_chunk(session, bucket, staging, *index, piece)?;
    *index += 1;
    Ok(())
}

fn bind_auth(
    session: &mut SessionState,
    req: &ParsedHttpRequest,
    credentials: &CredentialStore,
) -> Result<(), S3Reply> {
    let auth = match authenticate(req, credentials) {
        Ok(p) => p,
        Err((code, message)) => return Err(S3Reply::Error { code, message }),
    };
    session.namespace_bound = auth.namespace_bound;
    if let Some(ns) = auth.namespace {
        session.namespace = ns;
    }
    session.grants = auth.grants;
    let principal = auth.login;
    if session.connection_quota.is_none() {
        match spacestorage_tenancy::bind_namespace_session(
            session.tenancy.as_ref(),
            &principal,
            session.namespace.as_str(),
        ) {
            Ok(hold) => session.connection_quota = hold,
            Err(err) => {
                return Err(S3Reply::Error {
                    code: "AccessDenied".into(),
                    message: err.to_string(),
                });
            }
        }
    }
    if session.principal_hold.is_none() {
        match session.limit_services.try_hold_principal(&auth.access_key) {
            Ok(hold) => session.principal_hold = Some(hold),
            Err(e) => {
                return Err(S3Reply::Error {
                    code: "SlowDown".into(),
                    message: e.to_string(),
                });
            }
        }
    }
    session.principal = Some(auth.access_key);
    apply_query_headers(
        session,
        header_lookup(&req.headers, "x-spacestorage-quorum"),
        header_lookup(&req.headers, "x-spacestorage-timeout"),
    );
    session.requested_type = header_lookup(&req.headers, "x-spacestorage-type").map(str::to_string);
    session.requested_opts = req
        .headers
        .iter()
        .filter_map(|(k, v)| {
            k.to_ascii_lowercase()
                .strip_prefix("x-spacestorage-opt-")
                .map(|name| (name.to_string(), v.clone()))
        })
        .collect();
    Ok(())
}

fn handle_http(
    session: &mut SessionState,
    req: &ParsedHttpRequest,
    credentials: &CredentialStore,
    vhost_suffix: Option<&str>,
) -> S3Reply {
    if let Err(e) = bind_auth(session, req, credentials) {
        return e;
    }

    let host = header_lookup(&req.headers, "host").unwrap_or("localhost");
    let path = match rewrite_vhost(host, &req.path, vhost_suffix) {
        Ok(p) => p,
        Err(reply) => return reply,
    };
    let query = req.query.as_deref().map(parse_query).unwrap_or_default();
    let body = match prepare_body(req, credentials) {
        Ok(b) => b,
        Err(reply) => return reply,
    };

    if let Some(sub) = route_subresource(&req.method, &path, &query) {
        return dispatch(
            session,
            &sub.verb,
            &sub.args.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }

    if header_lookup(&req.headers, "x-amz-copy-source").is_some() && query.contains_key("uploadId")
    {
        return dispatch(session, "UPLOADPARTCOPY", &[]);
    }
    // SSE-KMS is refused by design (019 / `classify_s3`). Only the SSE header
    // routes here: a request-supplied `x-amz-checksum-crc64nvme` is an
    // *additional* checksum the server accepts but does not verify (we verify
    // content-md5 / sha256 / crc32 / crc32c), so it must NOT divert the whole
    // request into the SSE-KMS refusal — recent AWS SDKs/CLI send it by default
    // (`when_supported`) and mandatory for `DeleteObjects`.
    if header_lookup(&req.headers, "x-amz-server-side-encryption").is_some() {
        return dispatch(session, "SSEKMS", &[]);
    }

    if let Some(copy_source) = header_lookup(&req.headers, "x-amz-copy-source") {
        let (bucket, key) = split_bucket_key(&path);
        let b = bucket.as_deref().unwrap_or("bucket");
        let k = key.as_deref().unwrap_or("key");
        let directive = header_lookup(&req.headers, "x-amz-metadata-directive").unwrap_or("COPY");
        return copy_object(
            session,
            b,
            k,
            copy_source,
            directive,
            &meta_from_headers(&req.headers),
        );
    }

    let routed = match route_http(&req.method, &path, &query) {
        Ok(r) => r,
        Err(_) => {
            observe_s3("UNROUTED");
            return S3Reply::Error {
                code: "NotImplemented".into(),
                message: format!("{} not supported", req.method),
            };
        }
    };
    observe_s3(&routed.verb);

    match routed.verb.as_str() {
        "PUTOBJECT" => {
            if let Err(e) = crate::checksum::check_headers(&req.headers, &body) {
                return e;
            }
            let b = routed.args.first().map(String::as_str).unwrap_or("bucket");
            let k = routed.args.get(1).map(String::as_str).unwrap_or("key");
            if let Err(e) = reject_if_none_match_star(session, b, k, &req.headers) {
                return e;
            }
            put_object_with_meta(session, b, k, body, meta_from_headers(&req.headers))
        }
        "COMPLETEMULTIPARTUPLOAD" => {
            let upload_id = routed.args.first().map(String::as_str).unwrap_or("");
            let xml = String::from_utf8_lossy(&body).into_owned();
            dispatch(
                session,
                "COMPLETEMULTIPARTUPLOAD",
                &[upload_id, xml.as_str()],
            )
        }
        "UPLOADPART" => {
            if let Err(e) = crate::checksum::check_headers(&req.headers, &body) {
                return e;
            }
            let upload_id = routed.args.first().map(String::as_str).unwrap_or("");
            let part_number = query
                .get("partNumber")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);
            upload_part_bytes(session, upload_id, part_number, body)
        }
        "DELETEOBJECTS" => {
            let b = routed.args.first().map(String::as_str).unwrap_or("bucket");
            let text = String::from_utf8_lossy(&body).into_owned();
            dispatch(session, "DELETEOBJECTS", &[b, &text])
        }
        "GETOBJECT" | "HEADOBJECT" => {
            let args: Vec<&str> = routed.args.iter().map(String::as_str).collect();
            let reply = dispatch(session, &routed.verb, &args);
            apply_get_modifiers(reply, &req.headers, &query, routed.verb == "HEADOBJECT")
        }
        _ => {
            let args: Vec<&str> = routed.args.iter().map(String::as_str).collect();
            dispatch(session, &routed.verb, &args)
        }
    }
}

fn meta_from_headers(headers: &[(String, String)]) -> ObjectMeta {
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
        } else if let Some(name) = k.strip_prefix("x-amz-meta-") {
            meta.user_meta.push((name.to_string(), v.clone()));
        } else if let Some(name) = k.strip_prefix("X-Amz-Meta-") {
            meta.user_meta.push((name.to_string(), v.clone()));
        }
    }
    meta
}

fn prepare_body(
    req: &ParsedHttpRequest,
    credentials: &CredentialStore,
) -> Result<Vec<u8>, S3Reply> {
    let mode = header_lookup(&req.headers, "x-amz-content-sha256").unwrap_or("");
    if mode == "STREAMING-AWS4-HMAC-SHA256-PAYLOAD" || mode == "STREAMING-UNSIGNED-PAYLOAD-TRAILER"
    {
        let decoded = decode_aws_chunked(&req.body).map_err(|e| S3Reply::Error {
            code: "InvalidArgument".into(),
            message: e,
        })?;
        if mode == "STREAMING-AWS4-HMAC-SHA256-PAYLOAD" {
            if let Err(reply) =
                verify_stream_signatures(req, &decoded.chunk_signatures, credentials)
            {
                return Err(reply);
            }
        }
        return Ok(decoded.data);
    }
    if mode.len() == 64 && mode.chars().all(|c| c.is_ascii_hexdigit()) {
        let got = sha256_hex_bytes(&req.body);
        if !got.eq_ignore_ascii_case(mode) {
            return Err(S3Reply::Error {
                code: "XAmzContentSHA256Mismatch".into(),
                message: "payload hash mismatch".into(),
            });
        }
    }
    Ok(req.body.clone())
}

fn verify_stream_signatures(
    req: &ParsedHttpRequest,
    chunks: &[(Vec<u8>, String)],
    credentials: &CredentialStore,
) -> Result<(), S3Reply> {
    let authorization = header_lookup(&req.headers, "authorization").unwrap_or("");
    let parsed = parse_authorization(authorization).map_err(|_| S3Reply::Error {
        code: "SignatureDoesNotMatch".into(),
        message: "bad authorization".into(),
    })?;
    let Some(cred) = credentials.lookup(&parsed.access_key) else {
        return Err(S3Reply::Error {
            code: "InvalidAccessKeyId".into(),
            message: "Unknown access key".into(),
        });
    };
    let amz_date = header_lookup(&req.headers, "x-amz-date").unwrap_or("");
    let scope = format!(
        "{}/{}/{}/aws4_request",
        parsed.date, parsed.region, parsed.service
    );
    let mut previous = parsed.signature;
    for (chunk, sig) in chunks {
        let sts = crate::sigv4::chunk_string_to_sign(amz_date, &scope, &previous, chunk);
        let expect = crate::sigv4::sign_chunk(
            &cred.secret,
            &parsed.date,
            &parsed.region,
            &parsed.service,
            &sts,
        );
        if expect != *sig {
            return Err(S3Reply::Error {
                code: "SignatureDoesNotMatch".into(),
                message: "chunk signature mismatch".into(),
            });
        }
        previous = expect;
    }
    let sts = crate::sigv4::chunk_string_to_sign(amz_date, &scope, &previous, b"");
    let _final = crate::sigv4::sign_chunk(
        &cred.secret,
        &parsed.date,
        &parsed.region,
        &parsed.service,
        &sts,
    );
    Ok(())
}

fn rewrite_vhost(host: &str, path: &str, suffix: Option<&str>) -> Result<String, S3Reply> {
    let Some(suffix) = suffix.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(path.to_string());
    };
    let host = host.split(':').next().unwrap_or(host);
    let suf = suffix.trim_start_matches('.');
    let Some(bucket) = host.strip_suffix(&format!(".{suf}")) else {
        return Ok(path.to_string());
    };
    if bucket.is_empty() || bucket.contains('.') {
        return Err(S3Reply::Error {
            code: "InvalidRequest".into(),
            message: "invalid virtual host".into(),
        });
    }
    let rest = path.trim_start_matches('/');
    if rest.is_empty() {
        Ok(format!("/{bucket}"))
    } else {
        Ok(format!("/{bucket}/{rest}"))
    }
}

fn precondition_error(
    stored: &[(String, String)],
    request: &[(String, String)],
) -> Option<S3Reply> {
    let etag = stored
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    if let Some(m) = header_lookup(request, "if-match") {
        if m != "*" && !m.split(',').any(|t| t.trim() == etag) {
            return Some(S3Reply::Error {
                code: "PreconditionFailed".into(),
                message: "If-Match failed".into(),
            });
        }
    }
    if let Some(m) = header_lookup(request, "if-none-match") {
        if m == "*" || m.split(',').any(|t| t.trim() == etag) {
            return Some(S3Reply::Error {
                code: "NotModified".into(),
                message: "Not Modified".into(),
            });
        }
    }
    if let Some(since) = header_lookup(request, "if-modified-since") {
        if let (Some(req), Some(obj)) = (
            crate::meta::parse_http_date(since),
            stored
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("last-modified"))
                .and_then(|(_, v)| crate::meta::parse_http_date(v)),
        ) {
            if obj <= req {
                return Some(S3Reply::Error {
                    code: "NotModified".into(),
                    message: "Not Modified".into(),
                });
            }
        }
    }
    if let Some(since) = header_lookup(request, "if-unmodified-since") {
        if let (Some(req), Some(obj)) = (
            crate::meta::parse_http_date(since),
            stored
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("last-modified"))
                .and_then(|(_, v)| crate::meta::parse_http_date(v)),
        ) {
            if obj > req {
                return Some(S3Reply::Error {
                    code: "PreconditionFailed".into(),
                    message: "If-Unmodified-Since failed".into(),
                });
            }
        }
    }
    None
}

fn apply_response_overrides(hdrs: &mut Vec<(String, String)>, query: &HashMap<String, String>) {
    for (q, h) in [
        ("response-content-type", "Content-Type"),
        ("response-cache-control", "Cache-Control"),
        ("response-content-disposition", "Content-Disposition"),
        ("response-content-encoding", "Content-Encoding"),
    ] {
        if let Some(v) = query_get(query, q) {
            if let Some(slot) = hdrs.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(h)) {
                slot.1 = v.to_string();
            } else {
                hdrs.push((h.into(), v.to_string()));
            }
        }
    }
}

/// HEAD of a stored object, including a streamed manifest. Range and
/// `Content-Length` come from the stored length; the body is not allocated.
fn head_from_stored_length(
    content_length: u64,
    mut hdrs: Vec<(String, String)>,
    headers: &[(String, String)],
    query: &HashMap<String, String>,
) -> S3Reply {
    if let Some(err) = precondition_error(&hdrs, headers) {
        return err;
    }
    apply_response_overrides(&mut hdrs, query);
    let mut len = content_length;
    if let Some(spec) =
        header_lookup(headers, "range").and_then(|r| r.trim().strip_prefix("bytes="))
    {
        let Some(stored) = usize::try_from(content_length).ok() else {
            return S3Reply::Error {
                code: "InvalidRange".into(),
                message: "invalid range".into(),
            };
        };
        let Some((start, end)) = parse_byte_range(spec, stored) else {
            return S3Reply::Error {
                code: "InvalidRange".into(),
                message: "invalid range".into(),
            };
        };
        len = (end - start) as u64;
        hdrs.push((
            "Content-Range".into(),
            format!("bytes {start}-{}/{content_length}", end.saturating_sub(1)),
        ));
    }
    if let Some(slot) = hdrs
        .iter_mut()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        slot.1 = len.to_string();
    } else {
        hdrs.push(("Content-Length".into(), len.to_string()));
    }
    S3Reply::Head {
        content_length: len,
        headers: hdrs,
    }
}

fn apply_get_modifiers(
    reply: S3Reply,
    headers: &[(String, String)],
    query: &HashMap<String, String>,
    is_head: bool,
) -> S3Reply {
    if let S3Reply::ChunkedObject {
        bucket,
        key,
        staging,
        total,
        chunk_count,
        window,
        headers: mut hdrs,
        mut status,
        ..
    } = reply
    {
        if let Some(err) = precondition_error(&hdrs, headers) {
            return err;
        }
        apply_response_overrides(&mut hdrs, query);
        let mut range = None;
        if let Some(spec) =
            header_lookup(headers, "range").and_then(|r| r.trim().strip_prefix("bytes="))
        {
            if let Some((start, end)) = parse_byte_range(spec, total as usize) {
                status = 206;
                hdrs.push((
                    "Content-Range".into(),
                    format!("bytes {start}-{}/{total}", end.saturating_sub(1)),
                ));
                if let Some(slot) = hdrs
                    .iter_mut()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                {
                    slot.1 = (end - start).to_string();
                }
                range = Some((start, end));
            } else {
                return S3Reply::Error {
                    code: "InvalidRange".into(),
                    message: "invalid range".into(),
                };
            }
        }
        if is_head {
            let len = range.map(|(s, e)| (e - s) as u64).unwrap_or(total);
            return S3Reply::Head {
                content_length: len,
                headers: hdrs,
            };
        }
        return S3Reply::ChunkedObject {
            bucket,
            key,
            staging,
            total,
            chunk_count,
            window,
            headers: hdrs,
            status,
            range,
        };
    }
    if let S3Reply::Head {
        content_length,
        headers: hdrs,
    } = reply
    {
        return head_from_stored_length(content_length, hdrs, headers, query);
    }
    let (mut bytes, mut hdrs, mut status) = match reply {
        S3Reply::Body(obj) => (obj.bytes, obj.headers, obj.status),
        other => return other,
    };
    if let Some(err) = precondition_error(&hdrs, headers) {
        return err;
    }
    apply_response_overrides(&mut hdrs, query);
    if let Some(range) = header_lookup(headers, "range") {
        if let Some(spec) = range.trim().strip_prefix("bytes=") {
            if let Some((start, end)) = parse_byte_range(spec, bytes.len()) {
                bytes = bytes[start..end].to_vec();
                status = 206;
                hdrs.push((
                    "Content-Range".into(),
                    format!(
                        "bytes {start}-{}/{len}",
                        end.saturating_sub(1),
                        len = {
                            hdrs.iter()
                                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, v)| v.parse::<usize>().ok())
                                .unwrap_or(end)
                        }
                    ),
                ));
            } else {
                return S3Reply::Error {
                    code: "InvalidRange".into(),
                    message: "invalid range".into(),
                };
            }
        }
    }
    if let Some(slot) = hdrs
        .iter_mut()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        slot.1 = bytes.len().to_string();
    }
    if is_head {
        let len = bytes.len() as u64;
        return S3Reply::Head {
            content_length: len,
            headers: hdrs,
        };
    }
    S3Reply::Body(ObjectReply {
        bytes,
        status,
        headers: hdrs,
    })
}

fn parse_byte_range(spec: &str, len: usize) -> Option<(usize, usize)> {
    let (start, end) = spec.split_once('-')?;
    if start.is_empty() {
        let n: usize = end.parse().ok()?;
        if n == 0 || n > len {
            return None;
        }
        return Some((len - n, len));
    }
    let start: usize = start.parse().ok()?;
    if start >= len {
        return None;
    }
    let end = if end.is_empty() {
        len
    } else {
        let e: usize = end.parse().ok()?;
        (e + 1).min(len)
    };
    if end <= start {
        return None;
    }
    Some((start, end))
}

fn append_amz_ids(head: &mut String, request_id: &str) {
    head.push_str("x-amz-request-id: ");
    head.push_str(request_id);
    head.push_str("\r\nx-amz-id-2: ");
    head.push_str(&x_amz_id_2(request_id));
    head.push_str("\r\n");
}

fn conn_line(keep_alive: bool) -> &'static str {
    if keep_alive {
        "Connection: keep-alive\r\n"
    } else {
        "Connection: close\r\n"
    }
}

async fn write_http_response<W: AsyncWrite + Unpin>(
    w: &mut W,
    session: &mut SessionState,
    reply: &S3Reply,
    request_id: &str,
    keep_alive: bool,
    resource: &str,
) -> std::io::Result<()> {
    if let S3Reply::ChunkedObject {
        bucket,
        staging,
        total,
        chunk_count,
        headers,
        status,
        range,
        ..
    } = reply
    {
        let (start, end) = range.unwrap_or((0, *total as usize));
        let mut head = format!("HTTP/1.1 {status} {}\r\n", status_text(*status));
        append_amz_ids(&mut head, request_id);
        for (k, v) in headers {
            if k.eq_ignore_ascii_case("content-length") {
                continue;
            }
            head.push_str(k);
            head.push_str(": ");
            head.push_str(v);
            head.push_str("\r\n");
        }
        head.push_str("Transfer-Encoding: chunked\r\n");
        head.push_str(conn_line(keep_alive));
        head.push_str("\r\n");
        w.write_all(head.as_bytes()).await?;
        let bucket = bucket.clone();
        let staging = staging.clone();
        let mut pos = 0usize;
        for index in 0..*chunk_count {
            let chunk = read_one_chunk(session, &bucket, &staging, index)?;
            let c_start = pos;
            let c_end = pos + chunk.len();
            pos = c_end;
            let lo = start.max(c_start);
            let hi = end.min(c_end);
            if lo < hi {
                let slice = &chunk[lo - c_start..hi - c_start];
                w.write_all(format!("{:x}\r\n", slice.len()).as_bytes())
                    .await?;
                w.write_all(slice).await?;
                w.write_all(b"\r\n").await?;
            }
            if c_end >= end {
                break;
            }
        }
        w.write_all(b"0\r\n\r\n").await?;
        return w.flush().await;
    }
    let bytes = match reply {
        S3Reply::Ok => {
            let mut head = String::from("HTTP/1.1 200 OK\r\n");
            append_amz_ids(&mut head, request_id);
            head.push_str("Content-Length: 0\r\n");
            head.push_str(conn_line(keep_alive));
            head.push_str("\r\n");
            head.into_bytes()
        }
        S3Reply::NoContent => {
            let mut head = String::from("HTTP/1.1 204 No Content\r\n");
            append_amz_ids(&mut head, request_id);
            head.push_str("Content-Length: 0\r\n");
            head.push_str(conn_line(keep_alive));
            head.push_str("\r\n");
            head.into_bytes()
        }
        S3Reply::OkHeaders(headers) => {
            let mut head = String::from("HTTP/1.1 200 OK\r\n");
            append_amz_ids(&mut head, request_id);
            for (k, v) in headers {
                head.push_str(&k);
                head.push_str(": ");
                head.push_str(&v);
                head.push_str("\r\n");
            }
            head.push_str("Content-Length: 0\r\n");
            head.push_str(conn_line(keep_alive));
            head.push_str("\r\n");
            head.into_bytes()
        }
        S3Reply::Xml(x) => {
            let mut head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\n",
                x.len()
            );
            append_amz_ids(&mut head, request_id);
            head.push_str(conn_line(keep_alive));
            head.push_str("\r\n");
            head.push_str(x);
            head.into_bytes()
        }
        S3Reply::Body(b) => {
            let status = status_text(b.status);
            let mut head = format!("HTTP/1.1 {} {status}\r\n", b.status);
            append_amz_ids(&mut head, request_id);
            for (k, v) in &b.headers {
                if k.eq_ignore_ascii_case("content-length") {
                    continue;
                }
                head.push_str(k);
                head.push_str(": ");
                head.push_str(v);
                head.push_str("\r\n");
            }
            head.push_str(&format!("Content-Length: {}\r\n", b.bytes.len()));
            head.push_str(conn_line(keep_alive));
            head.push_str("\r\n");
            let mut out = head.into_bytes();
            out.extend_from_slice(&b.bytes);
            out
        }
        S3Reply::Head {
            content_length,
            headers,
        } => {
            let mut head = String::from("HTTP/1.1 200 OK\r\n");
            append_amz_ids(&mut head, request_id);
            for (k, v) in headers {
                if k.eq_ignore_ascii_case("content-length") {
                    continue;
                }
                head.push_str(k);
                head.push_str(": ");
                head.push_str(v);
                head.push_str("\r\n");
            }
            head.push_str(&format!("Content-Length: {content_length}\r\n"));
            head.push_str(conn_line(keep_alive));
            head.push_str("\r\n");
            head.into_bytes()
        }
        S3Reply::Error { code, message } => xml_error_response(
            request_id,
            code,
            message,
            resource,
            status_for_code(code),
            keep_alive,
        )
        .into_bytes(),
        S3Reply::ChunkedObject { .. } => unreachable!("handled above"),
    };
    w.write_all(&bytes).await?;
    w.flush().await
}

fn status_for_code(code: &str) -> u16 {
    match code {
        "NoSuchKey" | "NoSuchUpload" | "NoSuchBucket" => 404,
        "AccessDenied"
        | "SignatureDoesNotMatch"
        | "InvalidAccessKeyId"
        | "RequestTimeTooSkewed" => 403,
        "compat_must_not" | "NotImplemented" => 501,
        "PreconditionFailed" => 412,
        "NotModified" => 304,
        "InvalidRange" => 416,
        "BucketNotEmpty" | "BucketAlreadyOwnedByYou" | "BucketAlreadyExists" => 409,
        "QuorumUnsatisfiable" | "SlowDown" => 503,
        "RequestTimeout" => 504,
        _ => 400,
    }
}

fn read_one_chunk(
    session: &mut SessionState,
    bucket: &str,
    staging: &str,
    index: u32,
) -> std::io::Result<Vec<u8>> {
    crate::dispatch::read_staged_chunk(session, bucket, staging, index).map_err(|e| {
        let message = match e {
            S3Reply::Error { message, .. } => message,
            _ => "chunk read failed".into(),
        };
        std::io::Error::new(std::io::ErrorKind::Other, message)
    })
}

fn xml_error_response(
    request_id: &str,
    code: &str,
    message: &str,
    resource: &str,
    status: u16,
    keep_alive: bool,
) -> String {
    let body = crate::xml::error_xml(code, message, resource, request_id);
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\n",
        reason = status_text(status)
    );
    append_amz_ids(&mut head, request_id);
    format!(
        "{head}Content-Type: application/xml\r\nContent-Length: {}\r\n{}\r\n{body}",
        body.len(),
        conn_line(keep_alive)
    )
}

fn status_text(status: u16) -> &'static str {
    match status {
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        412 => "Precondition Failed",
        416 => "Range Not Satisfiable",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        206 => "Partial Content",
        304 => "Not Modified",
        200 => "OK",
        _ => "Bad Request",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sigv4::{DEMO_ACCESS_KEY, DEMO_SECRET_KEY, SigV4Request, sign_request};
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn sha256_hex(data: &[u8]) -> String {
        hex::encode(Sha256::digest(data))
    }

    fn build_signed_put(path: &str, body: &[u8], host: &str, unsigned: bool) -> String {
        let (amz_date, credential_date) = crate::sigv4::amz_now();
        let payload_hash = if unsigned {
            "UNSIGNED-PAYLOAD".into()
        } else {
            sha256_hex(body)
        };
        let headers = [
            ("host", host),
            ("x-amz-date", amz_date.as_str()),
            ("x-amz-content-sha256", payload_hash.as_str()),
        ];
        let req = SigV4Request {
            method: "PUT",
            uri_path: path,
            query: None,
            headers: &headers,
            payload_hash: &payload_hash,
        };
        let auth = sign_request(
            DEMO_SECRET_KEY,
            DEMO_ACCESS_KEY,
            &req,
            &amz_date,
            credential_date.as_str(),
            "us-east-1",
            "s3",
            &["host", "x-amz-date", "x-amz-content-sha256"],
        );
        format!(
            "PUT {path} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz_date}\r\nx-amz-content-sha256: {payload_hash}\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    }

    async fn spawn_s3_listener(
        handler: S3Handler,
        cancel: CancellationToken,
    ) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    accept = listener.accept() => {
                        let Ok((sock, _)) = accept else { break };
                    let h = S3Handler {
                        catalog: Arc::clone(&handler.catalog),
                        profile: handler.profile,
                        namespace: handler.namespace.clone(),
                        limit_services: handler.limit_services.clone(),
                        tenancy: handler.tenancy.clone(),
                        region: handler.region.clone(),
                        virtual_host_suffix: handler.virtual_host_suffix.clone(),
                        multipart_idle: handler.multipart_idle,
                        multipart_min_part: handler.multipart_min_part,
                        multipart_max_parts: handler.multipart_max_parts,
                        credentials: Arc::clone(&handler.credentials),
                        multipart_registry: Arc::clone(&handler.multipart_registry),
                    };
                        let c = cancel.child_token();
                        tokio::spawn(async move { h.serve(sock, c).await });
                    }
                }
            }
        });
        addr
    }

    #[tokio::test]
    async fn http_smoke_create_put_get_demo_header() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = "PUT /smoke HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut resp = vec![0u8; 4096];
        let n = stream.read(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp[..n]);
        assert!(text.contains("200 OK"), "{text}");

        let mut stream2 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let body = b"hello";
        let req2 = format!(
            "PUT /smoke/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream2.write_all(req2.as_bytes()).await.unwrap();
        stream2.write_all(body).await.unwrap();
        let mut resp2 = vec![0u8; 4096];
        let n2 = stream2.read(&mut resp2).await.unwrap();
        assert!(String::from_utf8_lossy(&resp2[..n2]).contains("200 OK"));

        let mut stream3 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req3 = "GET /smoke/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n";
        stream3.write_all(req3.as_bytes()).await.unwrap();
        let mut resp3 = vec![0u8; 4096];
        let n3 = stream3.read(&mut resp3).await.unwrap();
        let text3 = String::from_utf8_lossy(&resp3[..n3]);
        assert!(text3.contains("200 OK"));
        assert!(text3.contains("hello"));

        cancel.cancel();
    }

    #[tokio::test]
    async fn http_smoke_sigv4_put_get() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;

        let host = format!("127.0.0.1:{}", addr.port());
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = build_signed_put("/sig/b", b"data", &host, true);
        stream.write_all(req.as_bytes()).await.unwrap();
        stream.write_all(b"data").await.unwrap();
        let mut resp = vec![0u8; 4096];
        let n = stream.read(&mut resp).await.unwrap();
        assert!(String::from_utf8_lossy(&resp[..n]).contains("200 OK"));
        cancel.cancel();
    }

    async fn demo_get(path_and_query: &str) -> String {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!(
            "GET {path_and_query} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut resp = vec![0u8; 4096];
        let n = stream.read(&mut resp).await.unwrap();
        cancel.cancel();
        String::from_utf8_lossy(&resp[..n]).into_owned()
    }

    async fn demo_delete(path_and_query: &str) -> String {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!(
            "DELETE {path_and_query} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut resp = vec![0u8; 4096];
        let n = stream.read(&mut resp).await.unwrap();
        cancel.cancel();
        String::from_utf8_lossy(&resp[..n]).into_owned()
    }

    #[tokio::test]
    async fn keyless_get_lists_objects_v1() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"PUT /mybucket HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let _ = stream.read(&mut [0u8; 4096]).await.unwrap();
        let mut stream2 = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream2
            .write_all(b"GET /mybucket HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut resp = vec![0u8; 4096];
        let n = stream2.read(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp[..n]);
        assert!(text.contains("200 OK"), "{text}");
        assert!(text.contains("ListBucketResult"), "{text}");
        cancel.cancel();
    }

    #[tokio::test]
    async fn list_parts_and_abort_multipart_unknown_upload() {
        let list_parts = demo_get("/b/k?uploadId=up-1").await;
        assert!(list_parts.contains("NoSuchUpload"), "{list_parts}");

        let abort = demo_delete("/b/k?uploadId=up-1").await;
        assert!(abort.contains("NoSuchUpload"), "{abort}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn streaming_sigv4_put_uses_chunk_window() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;
        let host = format!("127.0.0.1:{}", addr.port());

        let mut bucket = tokio::net::TcpStream::connect(addr).await.unwrap();
        bucket
            .write_all(b"PUT /st HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let _ = read_tcp(&mut bucket).await;

        let payload = vec![b'q'; crate::stream::CHUNK_WINDOW + 32];
        let put = signed_streaming_put(&host, "/st/o", &payload);
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(put.as_bytes()).await.unwrap();
        let resp = String::from_utf8_lossy(&read_tcp(&mut stream).await).into_owned();
        assert!(resp.contains("200"), "{resp}");

        let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
        get.write_all(b"GET /st/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let got = read_tcp(&mut get).await;
        let text = String::from_utf8_lossy(&got);
        assert!(
            text.to_ascii_lowercase()
                .contains("transfer-encoding: chunked"),
            "streaming put above the window must be stored as pieces: {text}"
        );
        let start = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
        assert_eq!(decode_http_chunks(&got[start..]), payload);

        let mut framed = signed_streaming_put(&host, "/st/bad", &payload);
        if let Some(pos) = framed.find("chunk-signature=") {
            let sig_at = pos + "chunk-signature=".len();
            let flip = if framed.as_bytes()[sig_at] == b'0' {
                "1"
            } else {
                "0"
            };
            framed.replace_range(sig_at..sig_at + 1, flip);
        }
        let mut bad = tokio::net::TcpStream::connect(addr).await.unwrap();
        bad.write_all(framed.as_bytes()).await.unwrap();
        let bad_resp = String::from_utf8_lossy(&read_tcp(&mut bad).await).into_owned();
        assert!(bad_resp.contains("SignatureDoesNotMatch"), "{bad_resp}");
        let mut missing = tokio::net::TcpStream::connect(addr).await.unwrap();
        missing
            .write_all(b"GET /st/bad HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let missing_resp = String::from_utf8_lossy(&read_tcp(&mut missing).await).into_owned();
        assert!(missing_resp.contains("NoSuchKey"), "{missing_resp}");

        let unsigned = unsigned_trailer_put(&host, "/st/u", b"hello");
        let mut u = tokio::net::TcpStream::connect(addr).await.unwrap();
        u.write_all(unsigned.as_bytes()).await.unwrap();
        let uresp = String::from_utf8_lossy(&read_tcp(&mut u).await).into_owned();
        assert!(uresp.contains("200"), "{uresp}");
        cancel.cancel();
    }

    fn signed_streaming_put(host: &str, path: &str, body: &[u8]) -> String {
        use crate::sigv4::{SigV4Request, amz_now, chunk_string_to_sign, sign_chunk, sign_request};
        let (amz, date) = amz_now();
        let mode = "STREAMING-AWS4-HMAC-SHA256-PAYLOAD";
        let headers = [
            ("host", host),
            ("x-amz-date", amz.as_str()),
            ("x-amz-content-sha256", mode),
        ];
        let req = SigV4Request {
            method: "PUT",
            uri_path: path,
            query: None,
            headers: &headers,
            payload_hash: mode,
        };
        let auth = sign_request(
            DEMO_SECRET_KEY,
            DEMO_ACCESS_KEY,
            &req,
            &amz,
            &date,
            "us-east-1",
            "s3",
            &["host", "x-amz-content-sha256", "x-amz-date"],
        );
        let seed = auth.split("Signature=").nth(1).unwrap();
        let scope = format!("{date}/us-east-1/s3/aws4_request");
        let sig1 = sign_chunk(
            DEMO_SECRET_KEY,
            &date,
            "us-east-1",
            "s3",
            &chunk_string_to_sign(&amz, &scope, seed, body),
        );
        let sig0 = sign_chunk(
            DEMO_SECRET_KEY,
            &date,
            "us-east-1",
            "s3",
            &chunk_string_to_sign(&amz, &scope, &sig1, b""),
        );
        let mut framed =
            format!("{len:x};chunk-signature={sig1}\r\n", len = body.len()).into_bytes();
        framed.extend_from_slice(body);
        framed.extend_from_slice(format!("\r\n0;chunk-signature={sig0}\r\n\r\n").as_bytes());
        format!(
            "PUT {path} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz}\r\nx-amz-content-sha256: {mode}\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            framed.len(),
            String::from_utf8_lossy(&framed)
        )
    }

    fn unsigned_trailer_put(host: &str, path: &str, body: &[u8]) -> String {
        use crate::sigv4::{SigV4Request, amz_now, sign_request};
        let (amz, date) = amz_now();
        let mode = "STREAMING-UNSIGNED-PAYLOAD-TRAILER";
        let headers = [
            ("host", host),
            ("x-amz-date", amz.as_str()),
            ("x-amz-content-sha256", mode),
        ];
        let req = SigV4Request {
            method: "PUT",
            uri_path: path,
            query: None,
            headers: &headers,
            payload_hash: mode,
        };
        let auth = sign_request(
            DEMO_SECRET_KEY,
            DEMO_ACCESS_KEY,
            &req,
            &amz,
            &date,
            "us-east-1",
            "s3",
            &["host", "x-amz-content-sha256", "x-amz-date"],
        );
        let mut framed = format!("{:x}\r\n", body.len()).into_bytes();
        framed.extend_from_slice(body);
        framed.extend_from_slice(b"\r\n0\r\n\r\n");
        format!(
            "PUT {path} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz}\r\nx-amz-content-sha256: {mode}\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            framed.len(),
            String::from_utf8_lossy(&framed)
        )
    }

    async fn read_tcp(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        out
    }

    fn decode_http_chunks(body: &[u8]) -> Vec<u8> {
        let mut i = 0;
        let mut out = Vec::new();
        while i + 2 <= body.len() {
            let Some(line_end) = body[i..].windows(2).position(|w| w == b"\r\n") else {
                break;
            };
            let line = std::str::from_utf8(&body[i..i + line_end]).unwrap_or("");
            let Ok(size) = usize::from_str_radix(line.trim(), 16) else {
                break;
            };
            i += line_end + 2;
            if size == 0 {
                break;
            }
            if i + size > body.len() {
                break;
            }
            out.extend_from_slice(&body[i..i + size]);
            i += size + 2;
        }
        out
    }

    #[tokio::test]
    async fn mismatch_sends_400_and_closes() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(cat);
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();
        let mut resp = vec![0u8; 512];
        let n = stream.read(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp[..n]);
        assert!(text.contains("400 Bad Request"));
        assert!(text.contains("protocol mismatch"));
        cancel.cancel();
    }

    #[test]
    fn head_uses_stored_length_without_allocating_the_object() {
        let reply = S3Reply::Head {
            content_length: u64::MAX,
            headers: vec![
                ("Content-Length".into(), u64::MAX.to_string()),
                ("ETag".into(), "\"abc\"".into()),
            ],
        };
        let headers = vec![("range".into(), "bytes=0-99".into())];
        match apply_get_modifiers(reply, &headers, &HashMap::new(), true) {
            S3Reply::Head {
                content_length,
                headers,
            } => {
                assert_eq!(content_length, 100);
                let expected = format!("bytes 0-99/{}", u64::MAX);
                let range = headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-range"))
                    .map(|(_, v)| v.as_str());
                assert_eq!(range, Some(expected.as_str()));
                let len = headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .map(|(_, v)| v.as_str());
                assert_eq!(len, Some("100"));
            }
            other => panic!("{other:?}"),
        }
        let reply = S3Reply::Head {
            content_length: 1000,
            headers: vec![
                ("Content-Length".into(), "1000".into()),
                ("ETag".into(), "\"abc\"".into()),
            ],
        };
        let bad = vec![("range".into(), "bytes=5000-6000".into())];
        match apply_get_modifiers(reply, &bad, &HashMap::new(), true) {
            S3Reply::Error { code, .. } => assert_eq!(code, "InvalidRange"),
            other => panic!("{other:?}"),
        }
    }

    fn resp_header<'a>(resp: &'a str, name: &str) -> Option<&'a str> {
        let head = resp.split("\r\n\r\n").next().unwrap_or("");
        head.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.eq_ignore_ascii_case(name).then_some(v.trim())
        })
    }

    async fn demo_exchange(addr: std::net::SocketAddr, head: &str, body: &[u8]) -> String {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(head.as_bytes()).await.unwrap();
        if !body.is_empty() {
            stream.write_all(body).await.unwrap();
        }
        String::from_utf8_lossy(&read_tcp(&mut stream).await).into_owned()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn head_streamed_object_reports_stored_length() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;
        let payload = vec![b'h'; crate::stream::CHUNK_WINDOW + 8];
        let _ = demo_exchange(
            addr,
            "PUT /hd HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"",
        )
        .await;
        let put = format!(
            "PUT /hd/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        let stored = demo_exchange(addr, &put, &payload).await;
        assert!(stored.contains("200"), "{stored}");
        let head = demo_exchange(
            addr,
            "HEAD /hd/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
            b"",
        )
        .await;
        assert!(head.contains("200"), "{head}");
        let len = payload.len().to_string();
        assert_eq!(resp_header(&head, "content-length"), Some(len.as_str()));
        assert!(
            head.split("\r\n\r\n").nth(1).unwrap_or("").is_empty(),
            "{head}"
        );
        let ranged = demo_exchange(
            addr,
            "HEAD /hd/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nRange: bytes=0-3\r\nConnection: close\r\n\r\n",
            b"",
        )
        .await;
        assert_eq!(resp_header(&ranged, "content-length"), Some("4"));
        let expected = format!("bytes 0-3/{}", payload.len());
        assert_eq!(
            resp_header(&ranged, "content-range"),
            Some(expected.as_str())
        );
        assert!(
            ranged.split("\r\n\r\n").nth(1).unwrap_or("").is_empty(),
            "{ranged}"
        );
        cancel.cancel();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn upload_part_streams_windows_and_checksums() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;
        let _ = demo_exchange(
            addr,
            "PUT /up HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"",
        )
        .await;
        let created = demo_exchange(
            addr,
            "POST /up/obj?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"",
        )
        .await;
        let upload_id = created
            .split("<UploadId>")
            .nth(1)
            .and_then(|r| r.split("</UploadId>").next())
            .unwrap_or("")
            .to_string();
        assert!(!upload_id.is_empty(), "{created}");
        let payload = vec![b'z'; crate::stream::CHUNK_WINDOW + 17];
        let etag = crate::etag::single_part_etag(&payload);
        let md5 = crate::checksum::content_md5_base64(&payload);
        let put = format!(
            "PUT /up/obj?partNumber=1&uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-MD5: {md5}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        let part = demo_exchange(addr, &put, &payload).await;
        assert!(part.contains("200"), "{part}");
        assert!(part.contains(&etag), "{part}");
        let bad_md5 = crate::checksum::content_md5_base64(b"nope");
        let bad = format!(
            "PUT /up/obj?partNumber=2&uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-MD5: {bad_md5}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        let rejected = demo_exchange(addr, &bad, &payload).await;
        assert!(rejected.contains("BadDigest"), "{rejected}");
        let listed = demo_exchange(
            addr,
            &format!(
                "GET /up/obj?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n"
            ),
            b"",
        )
        .await;
        assert!(listed.contains("<PartNumber>1</PartNumber>"), "{listed}");
        assert!(!listed.contains("<PartNumber>2</PartNumber>"), "{listed}");
        let done = demo_exchange(
            addr,
            &format!(
                "POST /up/obj?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
            b"",
        )
        .await;
        assert!(done.contains("200"), "{done}");
        let got = demo_exchange(
            addr,
            "GET /up/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
            b"",
        )
        .await;
        let raw = got.as_bytes();
        let body = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| &raw[i + 4..])
            .unwrap_or(b"");
        assert_eq!(decode_http_chunks(body), payload);
        cancel.cancel();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn if_none_match_fails_closed_on_read_quorum() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;
        let first = demo_exchange(
            addr,
            "PUT /inn/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 4\r\nConnection: close\r\n\r\n",
            b"keep",
        )
        .await;
        assert!(first.contains("200"), "{first}");
        let blocked = demo_exchange(
            addr,
            "PUT /inn/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-spacestorage-quorum: TWO\r\nIf-None-Match: *\r\nContent-Length: 3\r\nConnection: close\r\n\r\n",
            b"new",
        )
        .await;
        assert!(blocked.contains("QuorumUnsatisfiable"), "{blocked}");
        let still = demo_exchange(
            addr,
            "GET /inn/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
            b"",
        )
        .await;
        assert!(still.contains("keep"), "{still}");
        let missed = demo_exchange(
            addr,
            "PUT /inn/absent HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-spacestorage-quorum: TWO\r\nIf-None-Match: *\r\nContent-Length: 1\r\nConnection: close\r\n\r\n",
            b"x",
        )
        .await;
        assert!(missed.contains("QuorumUnsatisfiable"), "{missed}");
        let absent = demo_exchange(
            addr,
            "GET /inn/absent HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
            b"",
        )
        .await;
        assert!(absent.contains("NoSuchKey"), "{absent}");
        cancel.cancel();
    }
}
