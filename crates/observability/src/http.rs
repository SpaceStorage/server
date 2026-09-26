//! Axum-oriented metrics HTTP responses (admin-http / optional metrics handler).

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::encode::{encode_prometheus, prometheus_content_type};
use crate::filter::filter_namespace;
use crate::sample::SeriesSnapshot;

pub fn prometheus_response(body: String) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, prometheus_content_type())],
        body,
    )
        .into_response()
}

pub fn encode_global(series: &[SeriesSnapshot]) -> String {
    encode_prometheus(series)
}

pub fn encode_tenant(series: &[SeriesSnapshot], namespace: &str) -> String {
    encode_prometheus(&filter_namespace(series, namespace))
}

/// 404 body codes for tenant path.
pub fn metrics_disabled() -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({
            "code": "metrics_disabled",
            "message": "namespace metrics scrape is not enabled",
        })),
    )
        .into_response()
}

pub fn unknown_namespace() -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({
            "code": "unknown_namespace",
            "message": "unknown namespace",
        })),
    )
        .into_response()
}
