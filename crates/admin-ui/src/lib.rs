//! Cerebro-like cluster map and Kibana-like console (009).

pub mod console;
pub mod map;
pub mod pages;

use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

pub use console::{ConsoleAuthz, ConsoleService};
pub use map::{
    ClusterMapView, ContainerMapEntry, MapAuthz, MapComposer, MapScope, MigrationProgress,
    NodeMapEntry, ReplicaMapEntry, ShardMapEntry, TopologySnapshot,
};

/// Error code when UI is requested on first-binary / slice-11 off.
pub const UI_INGEST_SLICE11_REQUIRED: &str = "UiIngestSlice11Required";

/// Application name for `005`/`008` labels.
pub const APPLICATION_UI: &str = "spacestorage-ui";

/// Interim UI auth: bearer token principal roles (until full `014` session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiPrincipal {
    pub id: String,
    pub cluster_admin: bool,
    pub metrics_read_cluster: bool,
    /// Namespaces where this principal is `NAMESPACE_ADMIN`.
    pub namespace_admin: Vec<String>,
    /// Containers with `WRITE` (ns/container pairs as "ns/container").
    pub write_containers: Vec<String>,
    /// Containers with `READ`.
    pub read_containers: Vec<String>,
    /// Containers with `CREATE` in namespace (namespace names).
    pub create_namespaces: Vec<String>,
}

impl UiPrincipal {
    pub fn cluster_admin(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            cluster_admin: true,
            metrics_read_cluster: true,
            namespace_admin: vec![],
            write_containers: vec![],
            read_containers: vec![],
            create_namespaces: vec![],
        }
    }

    pub fn namespace_admin(id: impl Into<String>, ns: impl Into<String>) -> Self {
        let ns = ns.into();
        Self {
            id: id.into(),
            cluster_admin: false,
            metrics_read_cluster: false,
            namespace_admin: vec![ns.clone()],
            write_containers: vec![],
            read_containers: vec![],
            create_namespaces: vec![ns],
        }
    }

    pub fn unprivileged(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            cluster_admin: false,
            metrics_read_cluster: false,
            namespace_admin: vec![],
            write_containers: vec![],
            read_containers: vec![],
            create_namespaces: vec![],
        }
    }

    pub fn map_authz(&self) -> MapAuthz {
        if self.cluster_admin || self.metrics_read_cluster {
            MapAuthz::Cluster
        } else if !self.namespace_admin.is_empty() {
            MapAuthz::Namespace(self.namespace_admin.clone())
        } else {
            MapAuthz::Forbidden
        }
    }

    pub fn console_authz(&self) -> ConsoleAuthz {
        ConsoleAuthz {
            cluster_admin: self.cluster_admin,
            namespace_admin: self.namespace_admin.clone(),
            write_containers: self.write_containers.clone(),
            read_containers: self.read_containers.clone(),
            create_namespaces: self.create_namespaces.clone(),
        }
    }
}

/// Shared state for mounted UI routes.
#[derive(Clone)]
pub struct AdminUiState {
    pub slice11_enabled: bool,
    pub map: MapComposer,
    pub console: ConsoleService,
    /// Resolve bearer → principal. Callers inject node token / authz.
    pub resolve_principal: std::sync::Arc<dyn Fn(&str) -> Option<UiPrincipal> + Send + Sync>,
}

impl AdminUiState {
    pub fn disabled() -> Self {
        Self {
            slice11_enabled: false,
            map: MapComposer::empty(),
            console: ConsoleService::new(false),
            resolve_principal: std::sync::Arc::new(|_| None),
        }
    }

    pub fn with_token(token: impl Into<String>, principal: UiPrincipal) -> Self {
        let expected = token.into();
        let p = principal;
        Self {
            slice11_enabled: true,
            map: MapComposer::empty(),
            console: ConsoleService::new(true),
            resolve_principal: std::sync::Arc::new(move |t| {
                if t.trim() == expected.trim() {
                    Some(p.clone())
                } else {
                    None
                }
            }),
        }
    }
}

fn slice11_required() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "code": UI_INGEST_SLICE11_REQUIRED })),
    )
        .into_response()
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "code": "unauthorized", "message": "invalid token" })),
    )
        .into_response()
}

fn forbidden() -> Response {
    (StatusCode::FORBIDDEN,).into_response()
}

fn bearer(headers: &axum::http::HeaderMap) -> &str {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .strip_prefix("Bearer ")
        .unwrap_or("")
}

async fn get_cluster_map(
    axum::extract::State(state): axum::extract::State<AdminUiState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    if !state.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = (state.resolve_principal)(bearer(&headers)) else {
        return unauthorized();
    };
    let authz = principal.map_authz();
    if matches!(authz, MapAuthz::Forbidden) {
        return forbidden();
    }
    let ns_filter = q.get("namespace").map(|s| s.as_str());
    match state.map.compose(&authz, ns_filter) {
        Ok(view) => Json(view).into_response(),
        Err(_) => forbidden(),
    }
}

async fn post_console_query(
    axum::extract::State(state): axum::extract::State<AdminUiState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !state.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = (state.resolve_principal)(bearer(&headers)) else {
        return unauthorized();
    };
    match state.console.query(&principal.console_authz(), &body) {
        Ok(v) => Json(v).into_response(),
        Err(e) => console_err(e),
    }
}

async fn post_console_config(
    axum::extract::State(state): axum::extract::State<AdminUiState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !state.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = (state.resolve_principal)(bearer(&headers)) else {
        return unauthorized();
    };
    match state.console.config(&principal.console_authz(), &body) {
        Ok(v) => Json(v).into_response(),
        Err(e) => console_err(e),
    }
}

async fn post_console_containers(
    axum::extract::State(state): axum::extract::State<AdminUiState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !state.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = (state.resolve_principal)(bearer(&headers)) else {
        return unauthorized();
    };
    match state.console.create_container(&principal.console_authz(), &body) {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => console_err(e),
    }
}

fn console_err(e: console::ConsoleError) -> Response {
    let status = match e.code() {
        "forbidden" => StatusCode::FORBIDDEN,
        "payload_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
        "unauthorized" => StatusCode::UNAUTHORIZED,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    (
        status,
        Json(json!({ "code": e.code(), "message": e.to_string() })),
    )
        .into_response()
}

async fn ui_cluster(
    axum::extract::State(state): axum::extract::State<AdminUiState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    if !state.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = (state.resolve_principal)(bearer(&headers)) else {
        return unauthorized();
    };
    if matches!(principal.map_authz(), MapAuthz::Forbidden) {
        return forbidden();
    }
    let interval = q
        .get("interval")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(2)
        .clamp(1, 10);
    Html(pages::cluster_map_html(interval)).into_response()
}

async fn ui_console(
    axum::extract::State(state): axum::extract::State<AdminUiState>,
    headers: axum::http::HeaderMap,
) -> Response {
    if !state.slice11_enabled {
        return slice11_required();
    }
    let Some(principal) = (state.resolve_principal)(bearer(&headers)) else {
        return unauthorized();
    };
    let az = principal.console_authz();
    if !az.cluster_admin && az.namespace_admin.is_empty() {
        return forbidden();
    }
    Html(pages::console_html()).into_response()
}

async fn ui_root(axum::extract::State(state): axum::extract::State<AdminUiState>) -> Response {
    if !state.slice11_enabled {
        return slice11_required();
    }
    Redirect::temporary("/ui/console").into_response()
}

async fn ui_asset(axum::extract::Path(name): axum::extract::Path<String>) -> Response {
    match name.as_str() {
        "cluster-map.js" => (
            [(header::CONTENT_TYPE, "application/javascript")],
            include_str!("assets/cluster-map.js"),
        )
            .into_response(),
        "console.js" => (
            [(header::CONTENT_TYPE, "application/javascript")],
            include_str!("assets/console.js"),
        )
            .into_response(),
        "console.css" => (
            [(header::CONTENT_TYPE, "text/css")],
            include_str!("assets/console.css"),
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Nested router for merge into node admin-http (same paths, own state).
pub fn ui_router(state: AdminUiState) -> Router {
    Router::new()
        .route("/ui", get(ui_root))
        .route("/ui/cluster", get(ui_cluster))
        .route("/ui/console", get(ui_console))
        .route("/ui/assets/{*name}", get(ui_asset))
        .route("/v1/cluster/map", get(get_cluster_map))
        .route("/v1/console/query", post(post_console_query))
        .route("/v1/console/config", post(post_console_config))
        .route("/v1/console/containers", post(post_console_containers))
        .with_state(state)
}

/// CLI hint text for `spacestorage ui`.
pub fn ui_cli_hint(admin_http_base: &str) -> String {
    format!(
        "admin-http: {admin_http_base}\n  map:     {admin_http_base}/ui/cluster\n  console: {admin_http_base}/ui/console\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_admin_map_authz() {
        assert!(matches!(
            UiPrincipal::cluster_admin("a").map_authz(),
            MapAuthz::Cluster
        ));
    }

    #[test]
    fn unprivileged_forbidden() {
        assert!(matches!(
            UiPrincipal::unprivileged("x").map_authz(),
            MapAuthz::Forbidden
        ));
    }
}
