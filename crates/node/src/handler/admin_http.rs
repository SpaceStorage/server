use crate::Node;
use crate::admin::AdminService;
use crate::admin::auth::check_bearer;
use crate::handler::ClientStream;
use crate::lifecycle::NodeState;
use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
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

use super::session_auth::{
    self, auth_mutate, auth_read, caller_audit_read, caller_cluster_admin, caller_namespace_scoped,
    clear_session_cookie, session_view_json, set_session_cookie, AuthCaller,
};
use super::ui_reads::{
    build_job_rows, build_monitoring_panel, build_namespace_detail, build_namespace_list_rows,
    build_node_detail, build_node_list_rows, build_overview_input, find_job_row, section_refusal,
};
use spacestorage_admin_ui::shell::{parse_limit, LimitError, PageParams};
use spacestorage_admin_ui::{
    compose_job_detail, compose_jobs_list, compose_monitoring_panel, compose_namespace_detail,
    compose_namespace_list, compose_node_detail, compose_nodes_list, merge_audit_page,
};
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
        .route("/v1/admin", post(admin_op))
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
        .route("/v1/auth/login", post(auth_login))
        .route("/v1/auth/session", get(auth_session))
        .route("/v1/auth/logout", post(auth_logout))
        .route("/v1/ui/overview", get(get_ui_overview))
        .route("/v1/ui/namespaces", get(get_ui_namespaces))
        .route("/v1/ui/namespaces/{name}", get(get_ui_namespace_detail))
        .route("/v1/ui/nodes", get(get_ui_nodes))
        .route("/v1/ui/nodes/{id}", get(get_ui_node_detail))
        .route("/v1/ui/monitoring", get(get_ui_monitoring))
        .route("/v1/ui/monitoring/namespaces/{name}", get(get_ui_monitoring_ns))
        .route("/v1/ui/jobs", get(get_ui_jobs))
        .route("/v1/ui/jobs/{id}", get(get_ui_job_detail))
        .route("/v1/ui/audit", get(get_ui_audit))
        .route("/ui", get(ui_root))
        .route("/ui/overview", get(ui_overview))
        .route("/ui/namespaces", get(ui_shell_page))
        .route("/ui/namespaces/{*tail}", get(ui_shell_page))
        .route("/ui/nodes", get(ui_shell_page))
        .route("/ui/nodes/{*tail}", get(ui_shell_page))
        .route("/ui/monitoring", get(ui_shell_page))
        .route("/ui/monitoring/namespaces/{*tail}", get(ui_shell_page))
        .route("/ui/jobs", get(ui_shell_page))
        .route("/ui/jobs/{*tail}", get(ui_shell_page))
        .route("/ui/audit", get(ui_shell_page))
        .route("/ui/map", get(ui_map_page))
        .route("/ui/browse", get(ui_browse_page))
        .route("/ui/cluster", get(ui_cluster_redirect))
        .route("/ui/console", get(ui_console_redirect))
        .route("/ui/assets/{*name}", get(ui_asset))
        .route("/v1/cluster/map", get(cluster_map))
        .route("/v1/console/query", post(console_query))
        .route("/v1/console/config", post(console_config))
        .route("/v1/console/containers", post(console_containers))
        .route(
            "/v1/ingest/kafka",
            get(ingest_kafka_list).post(ingest_kafka_add),
        )
        .route(
            "/v1/ingest/kafka/{id}",
            axum::routing::delete(ingest_kafka_delete),
        )
        .route("/v1/l0/containers", get(l0_list).post(l0_create))
        .route("/v1/l0/containers/{namespace}/{name}", get(l0_get))
        .route("/v1/legal/holds", post(legal_hold_place))
        .route(
            "/v1/legal/holds/{id}",
            axum::routing::delete(legal_hold_release),
        )
        .route("/v1/legal/erase", post(legal_erase))
        .route("/v1/cdc/streams", post(cdc_create))
        .route("/v1/compositions", post(composition_create))
        .route("/v1/billing/estimate", post(billing_estimate))
        .route("/v1/kms/status", get(kms_status))
        .fallback(fallback)
        .layer(DefaultBodyLimit::max(ADMIN_HTTP_BODY_LIMIT))
        .with_state(node)
}

fn admin_op_requires_cluster_admin(op: &AdminOp) -> bool {
    matches!(
        op,
        AdminOp::NamespaceList
            | AdminOp::NamespaceCreate { .. }
            | AdminOp::NamespaceRename { .. }
            | AdminOp::NamespaceDelete { .. }
            | AdminOp::QuotaReplace { .. }
            | AdminOp::Reload
            | AdminOp::Stop { .. }
            | AdminOp::RoleList
            | AdminOp::RoleBindingList { .. }
            | AdminOp::VoterReplace { .. }
            | AdminOp::VoterGrow { .. }
            | AdminOp::VoterShrink { .. }
            | AdminOp::JoinTokenMint { .. }
            | AdminOp::Membership
            | AdminOp::Drain { .. }
            | AdminOp::Undrain { .. }
    )
}

fn tenant_isolation_forbidden() -> ErrorBody {
    ErrorBody {
        code: "forbidden".into(),
        message: "not permitted for this principal".into(),
        details: serde_json::json!({}),
    }
}

fn session_may_read_namespace(caller: &AuthCaller, name: &str) -> bool {
    match caller {
        AuthCaller::AdminToken => true,
        AuthCaller::Session(s) if s.cluster_admin => true,
        AuthCaller::Session(s) => s.namespace_scopes.iter().any(|n| n == name),
    }
}

fn authorize_admin_op(caller: &AuthCaller, op: &AdminOp) -> Result<(), ErrorBody> {
    if caller_namespace_scoped(caller) && admin_op_requires_cluster_admin(op) {
        return Err(ErrorBody {
            code: "forbidden".into(),
            message: "operation requires CLUSTER_ADMIN".into(),
            details: serde_json::json!({}),
        });
    }
    match op {
        AdminOp::Leases { .. } if caller_namespace_scoped(caller) => {
            Err(tenant_isolation_forbidden())
        }
        AdminOp::NamespaceDescribe { name }
            if caller_namespace_scoped(caller) && !session_may_read_namespace(caller, name) =>
        {
            Err(tenant_isolation_forbidden())
        }
        AdminOp::AuditList { .. } if !caller_audit_read(caller) => Err(ErrorBody {
            code: "forbidden".into(),
            message: "audit_list requires AUDIT_READ or CLUSTER_ADMIN".into(),
            details: serde_json::json!({}),
        }),
        AdminOp::Drain { .. } | AdminOp::Undrain { .. } if !caller_cluster_admin(caller) => {
            Err(ErrorBody {
                code: "forbidden".into(),
                message: "drain requires CLUSTER_ADMIN".into(),
                details: serde_json::json!({}),
            })
        }
        _ => Ok(()),
    }
}

fn record_ui_api_request(node: &Node, status: StatusCode) {
    use spacestorage_observability::{help_for, LabelSet};
    let mut labels = LabelSet::new();
    let _ = labels.insert("application", spacestorage_admin_ui::APPLICATION_UI);
    let _ = labels.insert("protocol", "admin-http");
    node.metrics.registry().inc(
        "spacestorage_api_requests_total",
        labels.clone(),
        help_for("spacestorage_api_requests_total"),
        1,
    );
    if status.is_client_error() || status.is_server_error() {
        node.metrics.registry().inc(
            "spacestorage_api_request_errors_total",
            labels,
            help_for("spacestorage_api_request_errors_total"),
            1,
        );
    }
}

fn with_ui_json(node: &Node, resp: axum::response::Response) -> axum::response::Response {
    record_ui_api_request(node, resp.status());
    let mut r = with_headers(node, resp);
    spacestorage_admin_ui::apply_shell_security_headers(&mut r);
    r
}

async fn admin_op(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let admin_tok = admin_token_file_trim(&node);
    let caller = match auth_mutate(&headers, &node.sessions, &admin_tok).await {
        Ok(c) => c,
        Err(error) => return session_auth::csrf_forbidden_response(error),
    };
    let op: AdminOp = match serde_json::from_slice(&body) {
        Ok(op) => op,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    code: "unknown_op".into(),
                    message: error.to_string(),
                    details: serde_json::json!({}),
                }),
            )
                .into_response();
        }
    };
    if let Err(error) = authorize_admin_op(&caller, &op) {
        return (StatusCode::FORBIDDEN, Json(error)).into_response();
    }
    match AdminService::execute(&node, op).await {
        Ok(value) => with_headers(&node, Json(value).into_response()),
        Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
    }
}

async fn get_ui_overview(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if !node.admin_ui.read().unwrap().slice11_enabled {
        return slice11_required();
    }
    if let Err(e) = auth(&headers, &node).await {
        return with_ui_json(
            &node,
            (StatusCode::UNAUTHORIZED, Json(e)).into_response(),
        );
    }
    let Some(principal) = ui_principal(&node, &headers) else {
        return with_ui_json(
            &node,
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    code: "session_invalid".into(),
                    message: "session expired or unknown".into(),
                    details: serde_json::json!({}),
                }),
            )
                .into_response(),
        );
    };
  let path = "/ui/overview";
    if let Some(msg) = section_refusal(path, &principal) {
        return with_ui_json(
            &node,
            (
                StatusCode::FORBIDDEN,
                Json(ErrorBody {
                    code: "forbidden".into(),
                    message: msg.into(),
                    details: serde_json::json!({}),
                }),
            )
                .into_response(),
        );
    }
    let ns_filter = if principal.cluster_admin {
        None
    } else {
        Some(principal.namespace_admin.as_slice())
    };
    let mut input = build_overview_input(&node, ns_filter).await;
    if !principal.cluster_admin {
        spacestorage_admin_ui::filter_overview_for_namespaces(
            &mut input,
            &principal.namespace_admin,
        );
    }
    with_ui_json(
        &node,
        Json(spacestorage_admin_ui::compose_overview(&input)).into_response(),
    )
}

fn ui_limit_bad_request() -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            code: "limit_exceeded".into(),
            message: "limit must be 1..=100".into(),
            details: serde_json::json!({}),
        }),
    )
        .into_response()
}

fn ui_forbidden() -> axum::response::Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorBody {
            code: "forbidden".into(),
            message: "forbidden".into(),
            details: serde_json::json!({}),
        }),
    )
        .into_response()
}

async fn require_ui_session(
    node: &Arc<Node>,
    headers: &HeaderMap,
    path: &str,
) -> Result<spacestorage_admin_ui::UiPrincipal, axum::response::Response> {
    if !node.admin_ui.read().unwrap().slice11_enabled {
        return Err(slice11_required());
    }
    if let Err(e) = auth(headers, node).await {
        return Err(with_ui_json(node, (StatusCode::UNAUTHORIZED, Json(e)).into_response()));
    }
    let Some(principal) = ui_principal(node, headers) else {
        return Err(with_ui_json(
            node,
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    code: "session_invalid".into(),
                    message: "session expired or unknown".into(),
                    details: serde_json::json!({}),
                }),
            )
                .into_response(),
        ));
    };
    if let Some(msg) = section_refusal(path, &principal) {
        return Err(with_ui_json(
            node,
            (
                StatusCode::FORBIDDEN,
                Json(ErrorBody {
                    code: "forbidden".into(),
                    message: msg.into(),
                    details: serde_json::json!({}),
                }),
            )
                .into_response(),
        ));
    }
    Ok(principal)
}

fn page_params(q: &std::collections::HashMap<String, String>) -> Result<PageParams, LimitError> {
    let limit = parse_limit(q.get("limit").map(|s| s.as_str()))?;
    let cursor = q.get("cursor").cloned().filter(|c| !c.is_empty());
    Ok(PageParams { limit, cursor })
}

async fn get_ui_namespaces(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let principal = match require_ui_session(&node, &headers, "/ui/namespaces").await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let params = match page_params(&q) {
        Ok(p) => p,
        Err(LimitError::Exceeded) => return ui_limit_bad_request(),
    };
    let ns_filter = if principal.cluster_admin {
        None
    } else {
        Some(principal.namespace_admin.as_slice())
    };
    let filter_q = q.get("q").map(|s| s.as_str());
    let (rows, _) = build_namespace_list_rows(&node, ns_filter, filter_q).await;
    with_ui_json(
        &node,
        Json(compose_namespace_list(&params, rows)).into_response(),
    )
}

async fn get_ui_namespace_detail(
    State(node): State<Arc<Node>>,
    Path(name): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let path = format!("/ui/namespaces/{name}");
    let principal = match require_ui_session(&node, &headers, &path).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !principal.cluster_admin && !principal.namespace_admin.iter().any(|n| n == &name) {
        return with_ui_json(&node, ui_forbidden());
    }
    let params = match page_params(&q) {
        Ok(p) => p,
        Err(LimitError::Exceeded) => return ui_limit_bad_request(),
    };
    let Some((summary, record, containers)) = build_namespace_detail(&node, &name).await else {
        return with_ui_json(&node, ui_forbidden());
    };
    with_ui_json(
        &node,
        Json(compose_namespace_detail(&summary, &record, &params, containers)).into_response(),
    )
}

async fn get_ui_node_detail(
    State(node): State<Arc<Node>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> axum::response::Response {
    let path = format!("/ui/nodes/{id}");
    let principal = match require_ui_session(&node, &headers, &path).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let ns_filter = if principal.cluster_admin {
        None
    } else {
        Some(principal.namespace_admin.as_slice())
    };
    let Some((row, detail)) = build_node_detail(&node, &id).await else {
        return with_ui_json(&node, ui_forbidden());
    };
    if let Some(allowed) = ns_filter {
        let (rows, _) = build_node_list_rows(&node, Some(allowed)).await;
        if !rows.iter().any(|r| r.node_id == row.node_id || r.node_name == row.node_name) {
            return with_ui_json(&node, ui_forbidden());
        }
    }
    with_ui_json(
        &node,
        Json(compose_node_detail(&row, &detail)).into_response(),
    )
}

async fn get_ui_monitoring(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match require_ui_session(&node, &headers, "/ui/monitoring").await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !principal.cluster_admin {
        return with_ui_json(&node, ui_forbidden());
    }
    let (scope, ns, series, want) = build_monitoring_panel(&node, None);
    let body = compose_monitoring_panel(scope, ns.as_deref(), &series, &want);
    with_ui_json(&node, Json(body).into_response())
}

async fn get_ui_monitoring_ns(
    State(node): State<Arc<Node>>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> axum::response::Response {
    let path = format!("/ui/monitoring/namespaces/{name}");
    let principal = match require_ui_session(&node, &headers, &path).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !principal.cluster_admin && !principal.namespace_admin.iter().any(|n| n == &name) {
        return with_ui_json(&node, ui_forbidden());
    }
    let (scope, ns, series, want) = build_monitoring_panel(&node, Some(&name));
    let body = compose_monitoring_panel(scope, ns.as_deref(), &series, &want);
    with_ui_json(&node, Json(body).into_response())
}

async fn get_ui_jobs(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let principal = match require_ui_session(&node, &headers, "/ui/jobs").await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let params = match page_params(&q) {
        Ok(p) => p,
        Err(LimitError::Exceeded) => return ui_limit_bad_request(),
    };
    let ns_filter = if principal.cluster_admin {
        None
    } else {
        Some(principal.namespace_admin.as_slice())
    };
    let rows = build_job_rows(&node, ns_filter);
    with_ui_json(
        &node,
        Json(compose_jobs_list(&params, rows)).into_response(),
    )
}

async fn get_ui_job_detail(
    State(node): State<Arc<Node>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> axum::response::Response {
    let path = format!("/ui/jobs/{id}");
    let principal = match require_ui_session(&node, &headers, &path).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let ns_filter = if principal.cluster_admin {
        None
    } else {
        Some(principal.namespace_admin.as_slice())
    };
    let Some(row) = find_job_row(&node, &id, ns_filter) else {
        return with_ui_json(&node, ui_forbidden());
    };
    with_ui_json(
        &node,
        Json(compose_job_detail(&row)).into_response(),
    )
}

async fn get_ui_audit(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let _principal = match require_ui_session(&node, &headers, "/ui/audit").await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let params = match page_params(&q) {
        Ok(p) => p,
        Err(LimitError::Exceeded) => return ui_limit_bad_request(),
    };
    let list_params = spacestorage_authz::AuditListParams {
        limit: params.limit as usize,
        cursor: params.cursor.clone(),
        namespace: q.get("namespace").cloned(),
    };
    let (entries, next_cursor) = node.audit.list(list_params);
    let body = merge_audit_page(&params, params.limit, next_cursor, entries);
    with_ui_json(&node, Json(body).into_response())
}

async fn get_ui_nodes(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let principal = match require_ui_session(&node, &headers, "/ui/nodes").await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let params = match page_params(&q) {
        Ok(p) => p,
        Err(LimitError::Exceeded) => return ui_limit_bad_request(),
    };
    let ns_filter = if principal.cluster_admin {
        None
    } else {
        Some(principal.namespace_admin.as_slice())
    };
    let (mut rows, source) = build_node_list_rows(&node, ns_filter).await;
    if let Some(st) = q.get("status").map(|s| s.as_str()).filter(|s| !s.is_empty()) {
        rows.retain(|r| r.readiness == st);
    }
    if let Some(needle) = q.get("q").map(|s| s.as_str()).filter(|s| !s.is_empty()) {
        rows.retain(|r| r.node_name.contains(needle) || r.node_id.contains(needle));
    }
    with_ui_json(
        &node,
        Json(compose_nodes_list(&params, source, rows)).into_response(),
    )
}

fn admin_token_file_trim(node: &Node) -> String {
    let cfg = node.config.load();
    cfg.admin_token_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}

async fn auth(headers: &HeaderMap, node: &Node) -> Result<(), ErrorBody> {
    let tok = admin_token_file_trim(node);
    match auth_read(headers, &node.sessions, &tok).await {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn auth_post(headers: &HeaderMap, node: &Node) -> Result<(), ErrorBody> {
    let tok = admin_token_file_trim(node);
    match auth_mutate(headers, &node.sessions, &tok).await {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Build migrate AuthzContext from the caller principal (T063 / FR-010).
fn migrate_authz(headers: &HeaderMap, node: &Node) -> spacestorage_migrate::AuthzContext {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let cfg = node.config.load();
    let expected = cfg
        .admin_token_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    if check_bearer(token, expected.trim()) {
        return spacestorage_migrate::AuthzContext::cluster_admin();
    }
    if let Ok(store) = node.sessions.read() {
        if let Some(rec) = store.resolve(token) {
            return spacestorage_migrate::AuthzContext::from_principal(
                rec.cluster_admin,
                rec.cluster_admin, // session without admin has no MIGRATE grant yet
                rec.cluster_admin,
            );
        }
    }
    // Unauthenticated path is rejected by `auth` first; safe default refuse.
    spacestorage_migrate::AuthzContext::from_principal(false, false, false)
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
    let tok = admin_token_file_trim(&node);
    let caller = match auth_mutate(&headers, &node.sessions, &tok).await {
        Ok(c) => c,
        Err(e) => return session_auth::csrf_forbidden_response(e),
    };
    if !caller_cluster_admin(&caller) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                code: "forbidden".into(),
                message: "reload requires CLUSTER_ADMIN".into(),
                details: serde_json::json!({}),
            }),
        )
            .into_response();
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
    let tok = admin_token_file_trim(&node);
    let caller = match auth_mutate(&headers, &node.sessions, &tok).await {
        Ok(c) => c,
        Err(e) => return session_auth::csrf_forbidden_response(e),
    };
    if !caller_cluster_admin(&caller) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                code: "forbidden".into(),
                message: "stop requires CLUSTER_ADMIN".into(),
                details: serde_json::json!({}),
            }),
        )
            .into_response();
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

fn tenancy_usage_series(node: &Node) -> Vec<spacestorage_observability::SeriesSnapshot> {
    use spacestorage_observability::{LabelSet, SampleKind, SeriesSnapshot, help_for};
    use spacestorage_tenancy::QuotaUnit;
    let registry = node.tenancy.registry.lock();
    let mut out = Vec::new();
    for record in registry.list() {
        for unit in [QuotaUnit::Bytes, QuotaUnit::Objects, QuotaUnit::Connections] {
            let usage = node.tenancy.usage.get(record.id, unit, &None);
            let name = match unit {
                QuotaUnit::Bytes => "spacestorage_namespace_usage_bytes",
                QuotaUnit::Objects => "spacestorage_namespace_usage_objects",
                QuotaUnit::Connections => "spacestorage_namespace_connections",
                QuotaUnit::OpsPerSec => continue,
            };
            let mut labels = LabelSet::new();
            let _ = labels.insert("namespace", record.name.as_str());
            out.push(SeriesSnapshot {
                name: name.into(),
                labels,
                kind: SampleKind::Gauge,
                help: help_for(name),
                counter: None,
                gauge: Some(usage as f64),
                hist_bounds: None,
                hist_counts: None,
                hist_sum: None,
                hist_count: None,
            });
        }
    }
    for (ns_id, unit, count) in node.tenancy.usage.exceeded_labeled() {
        let Some(record) = registry.get_by_id(ns_id) else {
            continue;
        };
        let mut labels = LabelSet::new();
        let _ = labels.insert("namespace", record.name.as_str());
        let _ = labels.insert("unit", unit.as_str());
        out.push(SeriesSnapshot {
            name: "spacestorage_quota_exceeded_total".into(),
            labels,
            kind: SampleKind::Counter,
            help: help_for("spacestorage_quota_exceeded_total"),
            counter: Some(count),
            gauge: None,
            hist_bounds: None,
            hist_counts: None,
            hist_sum: None,
            hist_count: None,
        });
    }
    out
}

fn compat_must_not_series() -> Vec<spacestorage_observability::SeriesSnapshot> {
    spacestorage_compat::must_not_snapshot()
        .into_iter()
        .map(|(protocol, verb, count)| {
            let mut labels = spacestorage_observability::LabelSet::new();
            let _ = labels.insert("protocol", protocol);
            let _ = labels.insert("verb", verb);
            spacestorage_observability::SeriesSnapshot {
                name: "spacestorage_compat_must_not_total".into(),
                labels,
                kind: spacestorage_observability::SampleKind::Counter,
                help: spacestorage_observability::help_for("spacestorage_compat_must_not_total"),
                counter: Some(count),
                gauge: None,
                hist_bounds: None,
                hist_counts: None,
                hist_sum: None,
                hist_count: None,
            }
        })
        .collect()
}

async fn metrics(State(node): State<Arc<Node>>) -> impl IntoResponse {
    // 008 scrape plus 015 process-local MUST NOT counters (absolute; not re-incremented).
    let mut extra = compat_must_not_series();
    extra.extend(tenancy_usage_series(&node));
    let body = node
        .metrics
        .render_with_system_extra(&system_snapshot(&node), &extra);
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
    with_headers(
        &node,
        Json(serde_json::json!({ "jobs": jobs })).into_response(),
    )
}

async fn jobs_create(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth_post(&headers, &node).await {
        return session_auth::csrf_forbidden_response(e);
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
    let authz = migrate_authz(&headers, &node);
    match node
        .jobs
        .start(kind, uuid::Uuid::nil(), strategy, v, &authz)
    {
        Ok(job) => (
            StatusCode::ACCEPTED,
            with_headers(
                &node,
                Json(serde_json::json!({ "job": job })).into_response(),
            ),
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
        Ok(job) => with_headers(
            &node,
            Json(serde_json::json!({ "job": job })).into_response(),
        ),
        Err(e) => migrate_err_response(e),
    }
}

async fn job_cancel(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> axum::response::Response {
    let tok = admin_token_file_trim(&node);
    let caller = match auth_mutate(&headers, &node.sessions, &tok).await {
        Ok(c) => c,
        Err(e) => return session_auth::csrf_forbidden_response(e),
    };
    if !caller_cluster_admin(&caller) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                code: "forbidden".into(),
                message: "job cancel requires CLUSTER_ADMIN".into(),
                details: serde_json::json!({}),
            }),
        )
            .into_response();
    }
    let Ok(jid) = uuid::Uuid::parse_str(&id) else {
        return migrate_err_response(spacestorage_migrate::MigrateError::NotFound);
    };
    match node.jobs.cancel(jid) {
        Ok(job) => with_headers(
            &node,
            Json(serde_json::json!({ "job": job })).into_response(),
        ),
        Err(e) => migrate_err_response(e),
    }
}

async fn job_resume(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> axum::response::Response {
    let tok = admin_token_file_trim(&node);
    let caller = match auth_mutate(&headers, &node.sessions, &tok).await {
        Ok(c) => c,
        Err(e) => return session_auth::csrf_forbidden_response(e),
    };
    if !caller_cluster_admin(&caller) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                code: "forbidden".into(),
                message: "job resume requires CLUSTER_ADMIN".into(),
                details: serde_json::json!({}),
            }),
        )
            .into_response();
    }
    let Ok(jid) = uuid::Uuid::parse_str(&id) else {
        return migrate_err_response(spacestorage_migrate::MigrateError::NotFound);
    };
    match node.jobs.resume(jid) {
        Ok(job) => (
            StatusCode::ACCEPTED,
            with_headers(
                &node,
                Json(serde_json::json!({ "job": job })).into_response(),
            ),
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
    let authz = migrate_authz(&headers, &node);
    match node
        .jobs
        .start(kind, uuid::Uuid::nil(), strategy, v, &authz)
    {
        Ok(job) => (
            StatusCode::ACCEPTED,
            with_headers(
                &node,
                Json(serde_json::json!({ "job": job })).into_response(),
            ),
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
    if let Ok(store) = node.sessions.read() {
        if let Some(rec) = session_auth::resolve_session(&store, headers) {
            return Some(if rec.cluster_admin {
                spacestorage_admin_ui::UiPrincipal::cluster_admin(rec.login)
            } else if !rec.namespace_scopes.is_empty() {
                let mut p = spacestorage_admin_ui::UiPrincipal::unprivileged(rec.login);
                p.namespace_admin = rec.namespace_scopes.clone();
                p.create_namespaces = rec.namespace_scopes.clone();
                p
            } else {
                spacestorage_admin_ui::UiPrincipal::unprivileged(rec.login)
            });
        }
    }
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .strip_prefix("Bearer ")
        .unwrap_or("");
    if node
        .sessions
        .read()
        .ok()
        .and_then(|s| s.resolve(token))
        .is_some()
    {
        return None;
    }
    let ui = node.admin_ui.read().ok()?;
    (ui.resolve_principal)(token)
}

/// `014` AuthLogin: exchange static admin token (or password) for a SessionToken bearer.
async fn auth_login(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    // Accept either Authorization: Bearer <admin-token-file> or JSON {"token":"..."}.
    let hdr = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .strip_prefix("Bearer ")
        .unwrap_or("");
    let json_tok = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("token")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    let presented = if !hdr.is_empty() {
        hdr
    } else {
        json_tok.as_str()
    };
    let expected = match node.config.load().admin_token_file.as_ref() {
        Some(p) => std::fs::read_to_string(p).unwrap_or_default(),
        None => String::new(),
    };
    if !check_bearer(presented, expected.trim()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                code: "unauthorized".into(),
                message: "invalid admin token".into(),
                details: serde_json::json!({}),
            }),
        )
            .into_response();
    }
    let (tok, rec) = {
        let mut store = node.sessions.write().unwrap();
        store.issue(
            uuid::Uuid::now_v7(),
            "admin",
            true,
            1,
            std::time::Duration::from_secs(12 * 3600),
        )
    };
    let body = session_view_json(&rec, 12 * 3600);
    let mut resp = with_ui_json(&node, Json(body).into_response());
    set_session_cookie(resp.headers_mut(), &tok, 12 * 3600);
    resp
}

async fn auth_session(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let store = match node.sessions.read() {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    code: "session_invalid".into(),
                    message: "session expired or unknown".into(),
                    details: serde_json::json!({}),
                }),
            )
                .into_response();
        }
    };
    let Some(rec) = session_auth::resolve_session(&store, &headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                code: "session_invalid".into(),
                message: "session expired or unknown".into(),
                details: serde_json::json!({}),
            }),
        )
            .into_response();
    };
    with_ui_json(
        &node,
        Json(session_view_json(&rec, 12 * 3600)).into_response(),
    )
}

async fn auth_logout(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let admin_tok = admin_token_file_trim(&node);
    let caller = match auth_mutate(&headers, &node.sessions, &admin_tok).await {
        Ok(c) => c,
        Err(e) => return session_auth::csrf_forbidden_response(e),
    };
    if let AuthCaller::Session(rec) = caller {
        if let Ok(mut store) = node.sessions.write() {
            store.revoke_hash(&rec.token_hash);
        }
    }
    let mut resp = with_ui_json(&node, StatusCode::NO_CONTENT.into_response());
    clear_session_cookie(resp.headers_mut());
    resp
}

async fn cluster_map(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    {
        let ui = node.admin_ui.read().unwrap();
        if !ui.slice11_enabled {
            return slice11_required();
        }
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
    // Live compose from 011 membership, catalog, 010 jobs (004 replica ids when present).
    let snap = crate::handler::live_map::build_topology_snapshot(&node).await;
    {
        let mut ui = node.admin_ui.write().unwrap();
        ui.map.set_topology(snap);
    }
    let ui = node.admin_ui.read().unwrap().clone();
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
    if let Err(e) = auth_post(&headers, &node).await {
        return session_auth::csrf_forbidden_response(e);
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
    if let Err(e) = auth_post(&headers, &node).await {
        return session_auth::csrf_forbidden_response(e);
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
    if let Err(e) = auth_post(&headers, &node).await {
        return session_auth::csrf_forbidden_response(e);
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
    match ui
        .console
        .create_container(&principal.console_authz(), &body)
    {
        Ok(v) => (
            StatusCode::CREATED,
            with_headers(&node, Json(v).into_response()),
        )
            .into_response(),
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

fn shell_page_response(html: String) -> axum::response::Response {
    let mut resp = (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response();
    spacestorage_admin_ui::apply_shell_security_headers(&mut resp);
    resp
}

async fn ui_root(State(node): State<Arc<Node>>) -> axum::response::Response {
    if !node.admin_ui.read().unwrap().slice11_enabled {
        return slice11_required();
    }
    axum::response::Redirect::permanent("/ui/overview").into_response()
}

async fn ui_overview(State(node): State<Arc<Node>>) -> axum::response::Response {
    if !node.admin_ui.read().unwrap().slice11_enabled {
        return slice11_required();
    }
    shell_page_response(spacestorage_admin_ui::pages::shell_html(
        "/ui/overview",
        spacestorage_admin_ui::shell::ShellSection::Overview,
    ))
}

async fn ui_shell_page(
    State(node): State<Arc<Node>>,
    uri: axum::http::Uri,
) -> axum::response::Response {
    if !node.admin_ui.read().unwrap().slice11_enabled {
        return slice11_required();
    }
    let path = uri.path();
    let section =
        spacestorage_admin_ui::shell::ShellSection::from_path(path).unwrap_or(
            spacestorage_admin_ui::shell::ShellSection::Overview,
        );
    shell_page_response(spacestorage_admin_ui::pages::shell_html(path, section))
}

async fn ui_map_page(State(node): State<Arc<Node>>) -> axum::response::Response {
    if !node.admin_ui.read().unwrap().slice11_enabled {
        return slice11_required();
    }
    shell_page_response(spacestorage_admin_ui::pages::shell_html(
        "/ui/map",
        spacestorage_admin_ui::shell::ShellSection::Map,
    ))
}

async fn ui_browse_page(State(node): State<Arc<Node>>) -> axum::response::Response {
    if !node.admin_ui.read().unwrap().slice11_enabled {
        return slice11_required();
    }
    shell_page_response(spacestorage_admin_ui::pages::shell_html(
        "/ui/browse",
        spacestorage_admin_ui::shell::ShellSection::Browse,
    ))
}

async fn ui_cluster_redirect(
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let qs = q
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let loc = if qs.is_empty() {
        "/ui/map".to_string()
    } else {
        format!("/ui/map?{qs}")
    };
    axum::response::Redirect::permanent(&loc).into_response()
}

async fn ui_console_redirect(
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let qs = q
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let loc = if qs.is_empty() {
        "/ui/browse".to_string()
    } else {
        format!("/ui/browse?{qs}")
    };
    axum::response::Redirect::permanent(&loc).into_response()
}

async fn ui_asset(
    axum::extract::Path(name): axum::extract::Path<String>,
) -> axum::response::Response {
    let mut resp = match name.as_str() {
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
        "ui.css" => (
            [(header::CONTENT_TYPE, "text/css")],
            include_str!("../../../admin-ui/src/assets/ui.css"),
        )
            .into_response(),
        "shell.js" => (
            [(header::CONTENT_TYPE, "application/javascript")],
            spacestorage_admin_ui::pages::SHELL_JS,
        )
            .into_response(),
        "shell.css" => (
            [(header::CONTENT_TYPE, "text/css")],
            spacestorage_admin_ui::pages::SHELL_CSS,
        )
            .into_response(),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    spacestorage_admin_ui::apply_shell_security_headers(&mut resp);
    resp
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

fn ingest_authz(
    node: &Node,
    headers: &HeaderMap,
) -> Result<spacestorage_ingest::IngestAuthz, ErrorBody> {
    if let Some(p) = ui_principal(node, headers) {
        return Ok(spacestorage_ingest::IngestAuthz::from_principal(
            p.cluster_admin,
            &p.namespace_admin,
            &p.write_containers,
        ));
    }
    // Bootstrap: static admin token file ⇒ cluster-admin (interim until full 014 grants).
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let expected = node
        .config
        .load()
        .admin_token_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    if check_bearer(token, expected.trim()) {
        return Ok(spacestorage_ingest::IngestAuthz::cluster_admin());
    }
    Err(ErrorBody {
        code: "unauthorized".into(),
        message: "invalid token".into(),
        details: serde_json::json!({}),
    })
}

async fn ingest_kafka_list(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let authz = match ingest_authz(&node, &headers) {
        Ok(a) => a,
        Err(e) => return (StatusCode::UNAUTHORIZED, Json(e)).into_response(),
    };
    match node.ingest.kafka.list_for(&authz) {
        Ok(items) => with_headers(
            &node,
            Json(serde_json::json!({ "kafka": items })).into_response(),
        ),
        Err(e) => ingest_err(e),
    }
}

async fn ingest_kafka_add(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let authz = match ingest_authz(&node, &headers) {
        Ok(a) => a,
        Err(e) => return (StatusCode::UNAUTHORIZED, Json(e)).into_response(),
    };
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
    match node.ingest.kafka.add(decl, &authz).await {
        Ok(d) => (
            StatusCode::CREATED,
            with_headers(&node, Json(d).into_response()),
        )
            .into_response(),
        Err(e) => ingest_err(e),
    }
}

async fn ingest_kafka_delete(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> axum::response::Response {
    let authz = match ingest_authz(&node, &headers) {
        Ok(a) => a,
        Err(e) => return (StatusCode::UNAUTHORIZED, Json(e)).into_response(),
    };
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
    match node.ingest.kafka.delete(uid, &authz).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ingest_err(e),
    }
}

async fn l0_list(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let items = node.later.l0.lock().list();
    with_headers(
        &node,
        Json(serde_json::json!({ "containers": items })).into_response(),
    )
}

async fn l0_create(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let req: crate::later_surfaces::L0CreateBody = match serde_json::from_slice(&body) {
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
    match node
        .later
        .create_l0(&req.namespace, &req.name, &req.type_name, req.mode())
    {
        Ok(id) => (
            StatusCode::CREATED,
            with_headers(
                &node,
                Json(serde_json::json!({ "id": id, "namespace": req.namespace, "name": req.name, "type_name": req.type_name })).into_response(),
            ),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                code: "l0_create_failed".into(),
                message: e,
                details: serde_json::json!({}),
            }),
        )
            .into_response(),
    }
}

async fn l0_get(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    Path((namespace, name)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match node.later.describe_l0(&namespace, &name) {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                code: "not_found".into(),
                message: e,
                details: serde_json::json!({}),
            }),
        )
            .into_response(),
    }
}

async fn legal_hold_place(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let req: crate::later_surfaces::HoldBody = match serde_json::from_slice(&body) {
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
    let id = uuid::Uuid::now_v7();
    let hold = spacestorage_storage::LegalHold {
        id,
        namespace: req.namespace,
        container: req.container,
        key: req.key,
        reason: req.reason,
        created_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };
    let _ = node.later.place_hold(hold.clone());
    (
        StatusCode::CREATED,
        with_headers(&node, Json(hold).into_response()),
    )
        .into_response()
}

async fn legal_hold_release(
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
    match node.later.release_hold(uid) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                code: "not_found".into(),
                message: e,
                details: serde_json::json!({}),
            }),
        )
            .into_response(),
    }
}

async fn legal_erase(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let req: crate::later_surfaces::EraseBody = match serde_json::from_slice(&body) {
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
    let erase = spacestorage_storage::EraseRequest {
        namespace: req.namespace,
        container: req.container,
        key: req.key,
        requested_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };
    // Migrate job path uses the same LegalHoldStore gate (010 erase orchestration).
    match spacestorage_migrate::erase_with_legal(&node.later.legal, erase) {
        Ok(rec) => (
            StatusCode::OK,
            with_headers(&node, Json(rec).into_response()),
        )
            .into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(ErrorBody {
                code: "legal_erase_refused".into(),
                message: e.to_string(),
                details: serde_json::json!({}),
            }),
        )
            .into_response(),
    }
}

async fn cdc_create(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let req: crate::later_surfaces::CdcCreateBody = match serde_json::from_slice(&body) {
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
    match node
        .later
        .create_cdc(&req.namespace, &req.name, &req.source_container)
    {
        Ok(id) => (
            StatusCode::CREATED,
            with_headers(
                &node,
                Json(serde_json::json!({
                    "id": id,
                    "namespace": req.namespace,
                    "name": req.name,
                    "source_container": req.source_container,
                }))
                .into_response(),
            ),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                code: "cdc_create_failed".into(),
                message: e,
                details: serde_json::json!({}),
            }),
        )
            .into_response(),
    }
}

async fn composition_create(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let req: crate::later_surfaces::CompositionCreateBody = match serde_json::from_slice(&body) {
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
    let ns = req.namespace.clone();
    let name = req.name.clone();
    let kind = req.kind.clone();
    let (members, placement) = req.into_parts();
    match node
        .later
        .create_composition(&ns, &name, &kind, members, placement)
    {
        Ok(id) => (
            StatusCode::CREATED,
            with_headers(
                &node,
                Json(serde_json::json!({ "id": id, "namespace": ns, "name": name, "kind": kind }))
                    .into_response(),
            ),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                code: "composition_create_failed".into(),
                message: e,
                details: serde_json::json!({}),
            }),
        )
            .into_response(),
    }
}

async fn billing_estimate(
    State(node): State<Arc<Node>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let req: crate::later_surfaces::BillingEstimateBody = match serde_json::from_slice(&body) {
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
    let est = node.later.estimate_billing(&req.usage());
    with_headers(&node, Json(est).into_response())
}

async fn kms_status(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node).await {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    let cfg = node.config.load();
    let kind = if cfg.keys.external_kms.is_some() {
        "external_kms"
    } else if cfg.effective_master_key_file().is_some() {
        "master_key_file"
    } else {
        "unset"
    };
    with_headers(
        &node,
        Json(serde_json::json!({
            "provider": kind,
            "external_kms": cfg.keys.external_kms,
        }))
        .into_response(),
    )
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
