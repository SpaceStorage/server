use crate::admin::auth::check_bearer;
use crate::admin::AdminService;
use crate::lifecycle::NodeState;
use crate::Node;
use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use spacestorage_admin_proto::{AdminOp, ErrorBody};
use std::sync::Arc;
use tokio::net::TcpStream;
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

    async fn serve(&self, stream: TcpStream, cancel: CancellationToken) {
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
        .route("/metrics", get(metrics_404))
        .fallback(fallback)
        .with_state(node)
}

fn auth(headers: &HeaderMap, node: &Node) -> Result<(), ErrorBody> {
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
    if let Err(e) = auth(&headers, &node) {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match AdminService::execute(&node, AdminOp::Status).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn config(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node) {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match AdminService::execute(&node, AdminOp::Config).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn threads(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node) {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match AdminService::execute(&node, AdminOp::Threads).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn buffers(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node) {
        return (StatusCode::UNAUTHORIZED, Json(e)).into_response();
    }
    match AdminService::execute(&node, AdminOp::Buffers).await {
        Ok(v) => with_headers(&node, Json(v).into_response()),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn reload(State(node): State<Arc<Node>>, headers: HeaderMap) -> axum::response::Response {
    if let Err(e) = auth(&headers, &node) {
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
    if let Err(e) = auth(&headers, &node) {
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

async fn metrics_404() -> impl IntoResponse {
    // 008 US1: global /metrics for implemented paths (was 404 placeholder in 001).
    let body = spacestorage_observability::Metrics::default().render_prometheus();
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        body,
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
