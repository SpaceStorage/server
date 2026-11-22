//! UI session cookie, CSRF, redirects (018 quickstart §1–4).

use spacestorage_admin_ui::{AdminUiState, UiPrincipal};
use spacestorage_authz::SessionTokenStore;
use spacestorage_conformance::{boot_one_node, LAB_ADMIN_TOKEN};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[test]
fn session_token_resolves_cluster_admin_for_ui() {
    let mut store = SessionTokenStore::new();
    let (tok, _) = store.issue(
        uuid::Uuid::now_v7(),
        "admin",
        true,
        1,
        std::time::Duration::from_secs(60),
    );
    let store = Arc::new(std::sync::RwLock::new(store));
    let store2 = Arc::clone(&store);
    let ui = AdminUiState {
        slice11_enabled: true,
        map: spacestorage_admin_ui::MapComposer::empty(),
        console: spacestorage_admin_ui::ConsoleService::new(true),
        resolve_principal: Arc::new(move |t| {
            let Ok(s) = store2.read() else {
                return None;
            };
            s.resolve(t).map(|rec| {
                if rec.cluster_admin {
                    UiPrincipal::cluster_admin(rec.login)
                } else {
                    UiPrincipal::unprivileged(rec.login)
                }
            })
        }),
    };
    assert!(ui.resolve_principal.as_ref()(&tok).unwrap().cluster_admin);
    assert!(ui.resolve_principal.as_ref()("lab-admin-token").is_none());
}

async fn http_raw(port: u16, req: &str) -> (u16, String, Vec<String>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let headers: Vec<String> = text
        .lines()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    (status, text, headers)
}

fn parse_set_cookie(headers: &[String]) -> Option<String> {
    headers
        .iter()
        .find(|h| h.to_ascii_lowercase().starts_with("set-cookie:"))
        .map(|h| h.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
}

#[tokio::test]
async fn login_cookie_csrf_and_redirects() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let host = format!("127.0.0.1:{port}");

    let (st, body, _) = http_raw(
        port,
        &format!("GET /ui/overview HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert_eq!(st, 200);
    assert!(!body.contains("/v1/ui/"));
    assert!(body.contains("login-panel"));

    let login_req = format!(
        "POST /v1/auth/login HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer {LAB_ADMIN_TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
    );
    let (st, body, hdrs) = http_raw(port, &login_req).await;
    assert_eq!(st, 200);
    let cookie = parse_set_cookie(&hdrs).expect("set-cookie");
    assert!(cookie.contains("ss_session="));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("Secure"));
    assert!(cookie.contains("SameSite=Strict"));
    let json: serde_json::Value = serde_json::from_str(body.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert!(json.get("csrf_token").and_then(|v| v.as_str()).is_some());
    assert!(json.get("token").is_none());

    let sess_req = format!(
        "GET /v1/auth/session HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
    );
    let (st, _, _) = http_raw(port, &sess_req).await;
    assert_eq!(st, 200);

    let (st, _, hdrs) = http_raw(
        port,
        &format!("GET /ui HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert_eq!(st, 308);
    assert!(hdrs.iter().any(|h| h.contains("/ui/overview")));

    let (st, _, hdrs) = http_raw(
        port,
        &format!(
            "GET /ui/cluster?namespace=acme HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        ),
    )
    .await;
    assert_eq!(st, 308);
    assert!(hdrs.iter().any(|h| h.contains("/ui/map?namespace=acme")));

    let (st, _, hdrs) = http_raw(
        port,
        &format!("GET /ui/console HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert_eq!(st, 308);
    assert!(hdrs.iter().any(|h| h.contains("/ui/browse")));

    let csrf = json.get("csrf_token").and_then(|v| v.as_str()).unwrap();
    let admin_body = r#"{"op":"namespace_create","name":"csrf-probe"}"#;
    let no_origin = format!(
        "POST /v1/admin HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        admin_body.len(),
        admin_body
    );
    let (st, body, _) = http_raw(port, &no_origin).await;
    assert_eq!(st, 403);
    assert!(body.contains("csrf_rejected"));

    let cli_req = format!(
        "POST /v1/admin HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {LAB_ADMIN_TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        admin_body.len(),
        admin_body
    );
    let (st, _, _) = http_raw(port, &cli_req).await;
    assert!(st == 200 || st == 400);

    let foreign = format!(
        "POST /v1/admin HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nOrigin: https://evil.example\r\nX-SpaceStorage-CSRF: {csrf}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        admin_body.len(),
        admin_body
    );
    let (st, body, _) = http_raw(port, &foreign).await;
    assert_eq!(st, 403);
    assert!(body.contains("csrf_rejected"));

    let bad_csrf = format!(
        "POST /v1/admin HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nOrigin: http://{host}\r\nX-SpaceStorage-CSRF: deadbeef\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        admin_body.len(),
        admin_body
    );
    let (st, _, _) = http_raw(port, &bad_csrf).await;
    assert_eq!(st, 403);

    let console_body = r#"{"namespace":"acme","container":"x","limit":1}"#;
    let console_no_csrf = format!(
        "POST /v1/console/query HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        console_body.len(),
        console_body
    );
    let (st, body, _) = http_raw(port, &console_no_csrf).await;
    assert_eq!(st, 403);
    assert!(body.contains("csrf_rejected"));

    let good = format!(
        "POST /v1/admin HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nOrigin: http://{host}\r\nX-SpaceStorage-CSRF: {csrf}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        admin_body.len(),
        admin_body
    );
    let (st, _, _) = http_raw(port, &good).await;
    assert!(st == 200 || st == 400);

    let shell_js = include_str!("../../admin-ui/src/assets/shell.js");
    let map_js = include_str!("../../admin-ui/src/assets/cluster-map.js");
    let console_js = include_str!("../../admin-ui/src/assets/console.js");
    for (name, src) in [
        ("shell.js", shell_js),
        ("cluster-map.js", map_js),
        ("console.js", console_js),
    ] {
        assert!(
            !src.contains("sessionStorage"),
            "{name} must not use sessionStorage"
        );
    }

    lab.shutdown().await;
}

#[tokio::test]
async fn session_bearer_rejected_and_logout() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let host = format!("127.0.0.1:{port}");
    let login_req = format!(
        "POST /v1/auth/login HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer {LAB_ADMIN_TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
    );
    let (_, body, hdrs) = http_raw(port, &login_req).await;
    let cookie = parse_set_cookie(&hdrs).unwrap();
    let json: serde_json::Value = serde_json::from_str(body.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    let hex = cookie
        .split(';')
        .next()
        .unwrap()
        .split('=')
        .nth(1)
        .unwrap();

    let bearer_sess = format!(
        "POST /v1/admin HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {hex}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    let (st, body, _) = http_raw(port, &bearer_sess).await;
    assert_eq!(st, 401);
    assert!(body.contains("session_invalid"));

    let csrf = json.get("csrf_token").and_then(|v| v.as_str()).unwrap();
    let logout = format!(
        "POST /v1/auth/logout HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nOrigin: http://{host}\r\nX-SpaceStorage-CSRF: {csrf}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    let (st, _, _) = http_raw(port, &logout).await;
    assert_eq!(st, 204);
    let sess_req = format!(
        "GET /v1/auth/session HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
    );
    let (st, body, _) = http_raw(port, &sess_req).await;
    assert_eq!(st, 401);
    assert!(body.contains("session_invalid"));

    lab.shutdown().await;
}