//! Browser session cookie + CSRF for admin-http (018 session-cookie.md).

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use spacestorage_admin_proto::ErrorBody;
use spacestorage_authz::{csrf_tokens_equal, affordances_for_session, SessionToken, SessionTokenStore};
use std::sync::{Arc, RwLock};

pub const SESSION_COOKIE: &str = "ss_session";
pub const CSRF_HEADER: &str = "x-spacestorage-csrf";

#[derive(Debug, Clone)]
pub enum AuthCaller {
    AdminToken,
    Session(SessionToken),
}

pub fn parse_ss_session_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix(&format!("{SESSION_COOKIE}=")) {
            let val = v.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

pub fn resolve_session(
    store: &SessionTokenStore,
    headers: &HeaderMap,
) -> Option<SessionToken> {
    let cookie = parse_ss_session_cookie(headers)?;
    store.resolve_cookie(&cookie)
}

fn bearer_token(headers: &HeaderMap) -> &str {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .strip_prefix("Bearer ")
        .unwrap_or("")
}

pub async fn auth_read(
    headers: &HeaderMap,
    sessions: &Arc<RwLock<SessionTokenStore>>,
    admin_token: &str,
) -> Result<AuthCaller, ErrorBody> {
    let token = bearer_token(headers);
    if !token.is_empty() {
        if crate::admin::auth::check_bearer(token, admin_token) {
            return Ok(AuthCaller::AdminToken);
        }
        if sessions
            .read()
            .ok()
            .and_then(|s| s.resolve(token))
            .is_some()
        {
            return Err(ErrorBody {
                code: "session_invalid".into(),
                message: "session expired or unknown".into(),
                details: serde_json::json!({}),
            });
        }
        return Err(ErrorBody {
            code: "unauthorized".into(),
            message: "invalid token".into(),
            details: serde_json::json!({}),
        });
    }
    if let Ok(store) = sessions.read() {
        if let Some(rec) = resolve_session(&store, headers) {
            return Ok(AuthCaller::Session(rec));
        }
    }
    Err(ErrorBody {
        code: "session_invalid".into(),
        message: "session expired or unknown".into(),
        details: serde_json::json!({}),
    })
}

pub fn origin_ok(headers: &HeaderMap) -> bool {
    let origin = match headers.get(axum::http::header::ORIGIN).and_then(|v| v.to_str().ok()) {
        Some(o) => o,
        None => return false,
    };
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Ok(origin_url) = url_parse(origin) else {
        return false;
    };
    let Ok(host_url) = url_parse(&format!("http://{host}")) else {
        return false;
    };
    if origin_url.0 != host_url.0 || origin_url.1 != host_url.1 {
        return false;
    }
    let scheme = origin_url.2.as_str();
    if scheme == "https" {
        return true;
    }
    scheme == "http" && matches!(origin_url.0.as_str(), "localhost" | "127.0.0.1")
}

fn url_parse(s: &str) -> Result<(String, Option<String>, String), ()> {
    let (scheme, rest) = if let Some(r) = s.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = s.strip_prefix("http://") {
        ("http", r)
    } else {
        return Err(());
    };
    let (hostport, _) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = hostport
        .split_once(':')
        .map(|(h, p)| (h.to_string(), Some(p.to_string())))
        .unwrap_or((hostport.to_string(), None));
    Ok((host, port, scheme.to_string()))
}

pub async fn auth_mutate(
    headers: &HeaderMap,
    sessions: &Arc<RwLock<SessionTokenStore>>,
    admin_token: &str,
) -> Result<AuthCaller, ErrorBody> {
    let token = bearer_token(headers);
    if !token.is_empty() {
        if crate::admin::auth::check_bearer(token, admin_token) {
            return Ok(AuthCaller::AdminToken);
        }
        if sessions
            .read()
            .ok()
            .and_then(|s| s.resolve(token))
            .is_some()
        {
            return Err(ErrorBody {
                code: "session_invalid".into(),
                message: "session expired or unknown".into(),
                details: serde_json::json!({}),
            });
        }
        return Err(ErrorBody {
            code: "unauthorized".into(),
            message: "invalid token".into(),
            details: serde_json::json!({}),
        });
    }
    let Ok(store) = sessions.read() else {
        return csrf_err();
    };
    let Some(rec) = resolve_session(&store, headers) else {
        return Err(ErrorBody {
            code: "session_invalid".into(),
            message: "session expired or unknown".into(),
            details: serde_json::json!({}),
        });
    };
    if !origin_ok(headers) {
        return csrf_err();
    }
    let presented = headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !csrf_tokens_equal(&rec.csrf_token, presented) {
        return csrf_err();
    }
    Ok(AuthCaller::Session(rec))
}

fn csrf_err() -> Result<AuthCaller, ErrorBody> {
    Err(ErrorBody {
        code: "csrf_rejected".into(),
        message: "origin or csrf check failed".into(),
        details: serde_json::json!({}),
    })
}

pub fn session_view_json(rec: &SessionToken, ttl_secs: u64) -> serde_json::Value {
    let aff = affordances_for_session(rec);
    serde_json::json!({
        "expires_in": ttl_secs,
        "expires_at": serde_json::Value::Null,
        "principal": rec.login,
        "principal_id": rec.principal_id,
        "scope": if rec.cluster_admin { "cluster" } else { "namespace" },
        "namespaces": rec.namespace_scopes,
        "csrf_token": rec.csrf_token,
        "affordances": aff,
    })
}

pub fn set_session_cookie(headers: &mut axum::http::HeaderMap, token_hex: &str, max_age: i64) {
    let cookie = format!(
        "{SESSION_COOKIE}={token_hex}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={max_age}"
    );
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        headers.insert(axum::http::header::SET_COOKIE, v);
    }
}

pub fn clear_session_cookie(headers: &mut axum::http::HeaderMap) {
    let cookie = format!("{SESSION_COOKIE}=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0");
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        headers.insert(axum::http::header::SET_COOKIE, v);
    }
}

pub fn caller_cluster_admin(caller: &AuthCaller) -> bool {
    match caller {
        AuthCaller::AdminToken => true,
        AuthCaller::Session(s) => s.cluster_admin,
    }
}

/// Namespace-scoped session (not cluster admin, has tenant scopes).
pub fn caller_namespace_scoped(caller: &AuthCaller) -> bool {
    match caller {
        AuthCaller::AdminToken => false,
        AuthCaller::Session(s) => !s.cluster_admin && !s.namespace_scopes.is_empty(),
    }
}

pub fn caller_audit_read(caller: &AuthCaller) -> bool {
    match caller {
        AuthCaller::AdminToken => true,
        AuthCaller::Session(s) => {
            s.cluster_admin || spacestorage_authz::affordances_for_session(s).audit_read
        }
    }
}

pub fn csrf_forbidden_response(err: ErrorBody) -> axum::response::Response {
    let status = if err.code == "csrf_rejected" {
        StatusCode::FORBIDDEN
    } else if err.code == "session_invalid" {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::UNAUTHORIZED
    };
    (status, Json(err)).into_response()
}