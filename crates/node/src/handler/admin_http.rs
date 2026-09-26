use crate::admin::auth::check_bearer;
use crate::admin::AdminService;
use crate::handler::ClientStream;
use crate::lifecycle::NodeState;
use crate::Node;
use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use spacestorage_admin_proto::{AdminOp, ErrorBody};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tracing::debug;

use super::Handler;

pub struct AdminHttpHandler {
    pub node: Arc<Node>,
}

#[async_trait]
impl Handler for AdminHttpHandler {
    fn name(&self) -> &str {
        "admin-http"
    }

    fn kind(&self) -> &str {
        "admin"
    }

    async fn serve(&self, stream: ClientStream, cancel: CancellationToken) {
        let io = TokioIo::new(stream);
        let app = router(self.node.clone());
        let hyper_service = hyper::service::service_fn(move |req| {
            let app = app.clone();
            async move { app.oneshot(req).await }
        });

        tokio::select! {
            _ = cancel.cancelled() => {}
            r = http1::Builder::new().serve_connection(io, hyper_service) => {
                if let Err(e) = r {
                    debug!(error=%e, "admin-http connection error");
                }
            }
        }
    }
}

/// Admin-http request body ceiling (contracts/admin-http.md / T087).
const ADMIN_HTTP_BODY_LIMIT: usize = 1024 * 1024; // 1 MiB

fn router(node: Arc<Node>) -> Router {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/config", get(config))
        .route("/v1/threads", get(threads))
        .route("/v1/buffers", get(buffers))
        .route("/v1/reload", post(reload))
        .route("/v1/stop", post(stop))
        .route("/v1/health/live", get(live))
        .route("/v1/health/ready", get(ready))
        .route("/metrics", get(metrics))
        .route("/metrics/namespaces/{name}", get(metrics_namespace))
        .route("/v1/jobs", get(jobs_list).post(jobs_create))
        .route("/v1/jobs/{id}", get(job_status))
        .route("/v1/jobs/{id}/cancel", post(job_cancel))
        .route("/v1/jobs/{id}/resume", post(job_resume))
        .route("/v1/migrate", post(migrate_alias))
        .route("/v1/transform", post(transform_alias))
        .route("/v1/backup", post(backup_alias))
        .route("/v1/restore", post(restore_alias))
        .route("/v1/snapshots", get(snapshots_stub))
        .route("/ui", get(ui_root))
        .route("/ui/cluster", get(ui_cluster))
        .route("/ui/console", get(ui_console))
        .route("/ui/assets/{*name}", get(ui_asset))
        .route("/v1/cluster/map", get(cluster_map))
        .route("/v1/console/query", post(console_query))
        .route("/v1/console/config", post(console_config))
        .route("/v1/console/containers", post(console_containers))
        .route("/v1/ingest/kafka", get(ingest_kafka_list).post(ingest_kafka_add))
        .route("/v1/ingest/kafka/{id}", axum::routing::delete(ingest_kafka_delete))
        .fallback(fallback)
        .layer(DefaultBodyLimit::max(ADMIN_HTTP_BODY_LIMIT))
        .with_state(node)
}

async fn auth(headers: &HeaderMap, node: &Node) -> Result<(), ErrorBody> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let cfg = node.config.load();
    let expected = match cfg.admin_token_file.as_ref() {
        Some(p) => tokio::fs::read_to_string(p).await.unwrap_or_default(),
        None => String::new(),
    };
    if check_bearer(token, expected.trim()) {
        Ok(())
    } else {
        Err(ErrorBody {
            code: "unauthorized".into(),
            message: "invalid token".into(),
            details: serde_json::json!({}),
        })
    }
}

fn with_headers(node: &Node, mut resp: axum::response::Response) -> axum::response::Response {
    let cfg = node.config.load();
    if let Ok(v) = cfg.node_name.parse() {
        resp.headers_mut().insert("X-SpaceStorage-Node", v);
    }
    if let Ok(v) = node.state.get().as_str().parse() {
        resp.headers_mut().insert("X-SpaceStorage-State", v);
    }
    resp
}

async fn status(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match AdminService::execute(&node, AdminOp::Status).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn config(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match AdminService::execute(&node, AdminOp::Config).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn threads(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match AdminService::execute(&node, AdminOp::Threads).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn buffers(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match AdminService::execute(&node, AdminOp::Buffers).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn reload(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match AdminService::execute(&node, AdminOp::Reload).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn stop(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let wait = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("wait")?.as_bool())
        .unwrap_or(false);
    match AdminService::execute(&node, AdminOp::Stop { wait }).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn live(State(node): State<Arc<Node>>) -> axum::response::Response {
    let body = serde_json::json!({"state": node.state.get().as_str()});
    with_headers(&node, Json(body).into_response())
}

async fn ready(State(node): State<Arc<Node>>) -> axum::response::Response {
    let st = node.state.get();
    let code = if st == NodeState::Ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let body = serde_json::json!({"state": st.as_str()});
    let mut resp = (code, Json(body)).into_response();
    resp = with_headers(&node, resp);
    resp
}

fn system_snapshot(node: &Node) -> spacestorage_observability::NodeSystemSnapshot {
    let st = node.state.get();
    let ready = if st == NodeState::Ready { 1.0 } else { 0.0 };
    let buffers = node
        .buffers
        .reports()
        .into_iter()
        .map(|b| spacestorage_observability::BufferSnapshot {
            name: b.name,
            used_bytes: b.used_bytes,
            capacity_bytes: b.capacity_bytes,
            usage_ratio: b.usage_ratio,
            limit_hits_total: b.limit_hits_total,
        })
        .collect();
    spacestorage_observability::NodeSystemSnapshot {
        uptime_seconds: node.started_at.elapsed().as_secs_f64(),
        node_ready: ready,
        node_state: spacestorage_observability::node_state_value(st.as_str()),
        worker_threads: node.stats.workers() as f64,
        worker_threads_busy: node.stats.busy() as f64,
        buffers,
    }
}

async fn metrics(State(node): State<Arc<Node>>) -> impl IntoResponse {
    // 008: Four Golden Signals + system/billing series via registry encoder.
    let body = node.metrics.render_with_system(&system_snapshot(&node));
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            spacestorage_observability::prometheus_content_type(),
        )],
        body,
    )
}

async fn metrics_namespace(
    State(node): State<Arc<Node>>,
    Path(name): Path<String>,
) -> axum::response::Response {
    // Slice 9 tenant scrape: require enable bit; first-binary keeps route but 404s when disabled.
    if !node.metrics.tenant_scrape_enabled(&name) {
        return spacestorage_observability::metrics_disabled();
    }
    let snap = node.metrics.registry().snapshot();
    // Unknown namespace with scrape never enabled is already metrics_disabled; if enabled
    // but empty set still 200 with only that namespace's series (filter).
    let body = spacestorage_observability::encode_tenant(&snap, &name);
    spacestorage_observability::prometheus_response(body)
}

fn migrate_err_response(e: spacestorage_migrate::MigrateError) -> axum::response::Response {
    let code = match e.code() {
        "MigrateSlice10Required" => StatusCode::NOT_IMPLEMENTED,
        "AuthzDenied" => StatusCode::FORBIDDEN,
        "NotFound" => StatusCode::NOT_FOUND,
        "invalid_state" | "JobInProgress" => StatusCode::CONFLICT,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    (
        code,
        Json(ErrorBody {
            code: e.code().into(),
            message: e.to_string(),
            details: serde_json::json!({}),
        }),
    )
        .into_response()
}

async fn jobs_list(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let jobs = node.jobs.list();
    with_headers(&node, Json(serde_json::json!({ "jobs": jobs })).into_response())
}

async fn jobs_create(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let v: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    code: "bad_json".into(),
                    message: e.to_string(),
                    details: serde_json::json!({}),
                }),
            )
                .into_response();
        }
    };
    let kind_s = v
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or("data_migration");
    let kind = match spacestorage_migrate::JobKind::parse(kind_s) {
        Ok(k) => k,
        Err(e) => return migrate_err_response(e),
    };
    let strategy = match v
        .get("strategy")
        .and_then(|s| s.as_str())
        .map(spacestorage_migrate::Strategy::parse)
        .unwrap_or(Ok(spacestorage_migrate::Strategy::Live))
    {
        Ok(s) => s,
        Err(e) => return migrate_err_response(e),
    };
    let authz = spacestorage_migrate::AuthzContext::cluster_admin();
    match node.jobs.start(kind, uuid::Uuid::nil(), strategy, v, &authz) {
        Ok(job) => (
            StatusCode::ACCEPTED,
            with_headers(&node, Json(serde_json::json!({ "job": job })).into_response()),
        )
            .into_response(),
        Err(e) => migrate_err_response(e),
    }
}

async fn job_status(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let Ok(jid) = uuid::Uuid::parse_str(&id) else {
        return migrate_err_response(spacestorage_migrate::MigrateError::NotFound);
    };
    match node.jobs.status(jid) {
        Ok(job) => with_headers(&node, Json(serde_json::json!({ "job": job })).into_response()),
        Err(e) => migrate_err_response(e),
    }
}

async fn job_cancel(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let Ok(jid) = uuid::Uuid::parse_str(&id) else {
        return migrate_err_response(spacestorage_migrate::MigrateError::NotFound);
    };
    match node.jobs.cancel(jid) {
        Ok(job) => with_headers(&node, Json(serde_json::json!({ "job": job })).into_response()),
        Err(e) => migrate_err_response(e),
    }
}

async fn job_resume(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let Ok(jid) = uuid::Uuid::parse_str(&id) else {
        return migrate_err_response(spacestorage_migrate::MigrateError::NotFound);
    };
    match node.jobs.resume(jid) {
        Ok(job) => (
            StatusCode::ACCEPTED,
            with_headers(&node, Json(serde_json::json!({ "job": job })).into_response()),
        )
            .into_response(),
        Err(e) => migrate_err_response(e),
    }
}

async fn kind_alias(
    node: Arc<Node>,
    headers: HeaderMap,
    body: Bytes,
    kind: spacestorage_migrate::JobKind,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let mut v: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    code: "bad_json".into(),
                    message: e.to_string(),
                    details: serde_json::json!({}),
                }),
            )
                .into_response();
        }
    };
    if let Some(obj) = v.as_object_mut() {
        obj.insert("kind".into(), serde_json::json!(kind.as_str()));
    }
    let strategy = match v
        .get("strategy")
        .and_then(|s| s.as_str())
        .map(spacestorage_migrate::Strategy::parse)
        .unwrap_or(Ok(spacestorage_migrate::Strategy::Live))
    {
        Ok(s) => s,
        Err(e) => return migrate_err_response(e),
    };
    let authz = spacestorage_migrate::AuthzContext::cluster_admin();
    match node.jobs.start(kind, uuid::Uuid::nil(), strategy, v, &authz) {
        Ok(job) => (
            StatusCode::ACCEPTED,
            with_headers(&node, Json(serde_json::json!({ "job": job })).into_response()),
        )
            .into_response(),
        Err(e) => migrate_err_response(e),
    }
}

async fn migrate_alias(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    kind_alias(
        node,
        headers,
        body,
        spacestorage_migrate::JobKind::DataMigration,
    )
    .await
}

async fn transform_alias(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    kind_alias(
        node,
        headers,
        body,
        spacestorage_migrate::JobKind::DataTransformation,
    )
    .await
}

async fn backup_alias(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    kind_alias(
        node,
        headers,
        body,
        spacestorage_migrate::JobKind::DataBackup,
    )
    .await
}

async fn restore_alias(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    kind_alias(
        node,
        headers,
        body,
        spacestorage_migrate::JobKind::DataRestore,
    )
    .await
}

async fn snapshots_stub(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    if !node.jobs.slice10_enabled {
        return migrate_err_response(spacestorage_migrate::MigrateError::MigrateSlice10Required);
    }
    with_headers(
        &node,
        Json(serde_json::json!({ "snapshots": [] })).into_response(),
    )
}

fn slice11_required() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "code": "UiIngestSlice11Required" })),
    )
        .into_response()
}

fn ui_principal(node: &Node, headers: &HeaderMap) -> Option<spacestorage_admin_ui::UiPrincipal> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .strip_prefix("Bearer ")
        .unwrap_or("");
    let ui = node.admin_ui.read().ok()?;
    (ui.resolve_principal)(token)
}

async fn cluster_map(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let ui = node.admin_ui.read().unwrap().clone();
    if !ui.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = ui_principal(&node, &headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                code: "unauthorized".into(),
                message: "invalid token".into(),
                details: serde_json::json!({}),
            }),
        )
            .into_response();
    };
    let authz = principal.map_authz();
    if matches!(authz, spacestorage_admin_ui::MapAuthz::Forbidden) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let ns = q.get("namespace").map(|s| s.as_str());
    match ui.map.compose(&authz, ns) {
        Ok(view) => with_headers(&node, Json(view).into_response()),
        Err(_) => StatusCode::FORBIDDEN.into_response(),
    }
}

async fn console_query(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let ui = node.admin_ui.read().unwrap().clone();
    if !ui.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = ui_principal(&node, &headers) else {
        return (StatusCode::UNAUTHORIZED, Json(ErrorBody {
            code: "unauthorized".into(),
            message: "invalid token".into(),
            details: serde_json::json!({}),
        }))
            .into_response();
    };
    match ui.console.query(&principal.console_authz(), &body) {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => {
            let status = match e.code() {
                "forbidden" => StatusCode::FORBIDDEN,
                "payload_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            };
            (
                status,
                Json(serde_json::json!({ "code": e.code(), "message": e.to_string() })),
            )
                .into_response()
        }
    }
}

async fn console_config(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let ui = node.admin_ui.read().unwrap().clone();
    if !ui.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = ui_principal(&node, &headers) else {
        return (StatusCode::UNAUTHORIZED, Json(ErrorBody {
            code: "unauthorized".into(),
            message: "invalid token".into(),
            details: serde_json::json!({}),
        }))
            .into_response();
    };
    match ui.console.config(&principal.console_authz(), &body) {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => {
            let status = if e.code() == "forbidden" {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            (
                status,
                Json(serde_json::json!({ "code": e.code(), "message": e.to_string() })),
            )
                .into_response()
        }
    }
}

async fn console_containers(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let ui = node.admin_ui.read().unwrap().clone();
    if !ui.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = ui_principal(&node, &headers) else {
        return (StatusCode::UNAUTHORIZED, Json(ErrorBody {
            code: "unauthorized".into(),
            message: "invalid token".into(),
            details: serde_json::json!({}),
        }))
            .into_response();
    };
    match ui.console.create_container(&principal.console_authz(), &body) {
        Ok(v) => (StatusCode::CREATED, with_headers(&node, Json(v).into_response())).into_response(),
        Err(e) => {
            let status = if e.code() == "forbidden" {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            (
                status,
                Json(serde_json::json!({ "code": e.code(), "message": e.to_string() })),
            )
                .into_response()
        }
    }
}

async fn ui_root(State(node): State<Arc<Node>>) -> axum::response::Response {
    if !node.admin_ui.read().unwrap().slice11_enabled {
        return slice11_required();
    }
    axum::response::Redirect::temporary("/ui/console").into_response()
}

async fn ui_cluster(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let ui = node.admin_ui.read().unwrap().clone();
    if !ui.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = ui_principal(&node, &headers) else {
        return (StatusCode::UNAUTHORIZED, Json(ErrorBody {
            code: "unauthorized".into(),
            message: "invalid token".into(),
            details: serde_json::json!({}),
        }))
            .into_response();
    };
    if matches!(principal.map_authz(), spacestorage_admin_ui::MapAuthz::Forbidden) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let interval = q
        .get("interval")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(2)
        .clamp(1, 10);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        spacestorage_admin_ui::pages::cluster_map_html(interval),
    )
        .into_response()
}

async fn ui_console(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let ui = node.admin_ui.read().unwrap().clone();
    if !ui.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = ui_principal(&node, &headers) else {
        return (StatusCode::UNAUTHORIZED, Json(ErrorBody {
            code: "unauthorized".into(),
            message: "invalid token".into(),
            details: serde_json::json!({}),
        }))
            .into_response();
    };
    let az = principal.console_authz();
    if !az.cluster_admin && az.namespace_admin.is_empty() {
        return StatusCode::FORBIDDEN.into_response();
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        spacestorage_admin_ui::pages::console_html(),
    )
        .into_response()
}

async fn ui_asset(axum::extract::Path(name): axum::extract::Path<String>) -> axum::response::Response {
    match name.as_str() {
        "cluster-map.js" => (
            [(header::CONTENT_TYPE, "application/javascript")],
            include_str!("../../../admin-ui/src/assets/cluster-map.js"),
        )
            .into_response(),
        "console.js" => (
            [(header::CONTENT_TYPE, "application/javascript")],
            include_str!("../../../admin-ui/src/assets/console.js"),
        )
            .into_response(),
        "console.css" => (
            [(header::CONTENT_TYPE, "text/css")],
            include_str!("../../../admin-ui/src/assets/console.css"),
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn ingest_err(e: spacestorage_ingest::IngestError) -> axum::response::Response {
    let status = match e.code() {
        "UiIngestSlice11Required" => StatusCode::NOT_FOUND,
        "ingest_write_only" | "ingest_forbidden" | "ingest_syslog_cluster_only" => {
            StatusCode::FORBIDDEN
        }
        "not_found" => StatusCode::NOT_FOUND,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    (
        status,
        Json(serde_json::json!({ "code": e.code(), "message": e.to_string() })),
    )
        .into_response()
}

async fn ingest_kafka_list(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match node.ingest.kafka.list() {
        Ok(items) => with_headers(&node, Json(serde_json::json!({ "kafka": items })).into_response()),
        Err(e) => ingest_err(e),
    }
}

async fn ingest_kafka_add(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let mut decl: spacestorage_ingest::KafkaIngest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    code: "bad_json".into(),
                    message: e.to_string(),
                    details: serde_json::json!({}),
                }),
            )
                .into_response();
        }
    };
    if decl.id.is_nil() {
        decl.id = uuid::Uuid::now_v7();
    }
    let authz = spacestorage_ingest::IngestAuthz::cluster_admin();
    match node.ingest.kafka.add(decl, &authz) {
        Ok(d) => (StatusCode::CREATED, with_headers(&node, Json(d).into_response())).into_response(),
        Err(e) => ingest_err(e),
    }
}

async fn ingest_kafka_delete(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let Ok(uid) = uuid::Uuid::parse_str(&id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                code: "bad_id".into(),
                message: "invalid uuid".into(),
                details: serde_json::json!({}),
            }),
        )
            .into_response();
    };
    let authz = spacestorage_ingest::IngestAuthz::cluster_admin();
    match node.ingest.kafka.delete(uid, &authz) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ingest_err(e),
    }
}

async fn fallback() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            code: "unknown_op".into(),
            message: "unknown path".into(),
            details: serde_json::json!({}),
        }),
    )
}
