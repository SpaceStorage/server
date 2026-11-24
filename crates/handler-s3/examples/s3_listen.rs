//! In-process S3 endpoint for Suite B. Does not boot the full daemon.
//!
//! Credentials: access key `demo`, secret `demo`, region `us-east-1`.
//! `S3_LISTEN` (default `0.0.0.0:19001`), `S3_VHOST_SUFFIX` (optional).

use std::sync::{Arc, RwLock};
use std::time::Duration;

use spacestorage_handler_s3::S3Handler;
use spacestorage_types::ContainerCatalog;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

fn parse_size(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let (num, mul) = if let Some(n) = raw.strip_suffix("GiB") {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = raw.strip_suffix("MiB") {
        (n, 1024 * 1024)
    } else if let Some(n) = raw.strip_suffix("KiB") {
        (n, 1024)
    } else {
        (raw, 1)
    };
    num.trim()
        .parse::<u64>()
        .ok()
        .map(|n| n.saturating_mul(mul))
}

#[tokio::main]
async fn main() {
    let listen = std::env::var("S3_LISTEN").unwrap_or_else(|_| "0.0.0.0:19001".into());
    let max_value = std::env::var("S3_MAX_VALUE")
        .ok()
        .and_then(|s| parse_size(&s))
        .unwrap_or(1024 * 1024 * 1024);
    let max_key = std::env::var("S3_MAX_KEY")
        .ok()
        .and_then(|s| parse_size(&s))
        .unwrap_or(4 * 1024);
    let suffix = std::env::var("S3_VHOST_SUFFIX")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut handler = S3Handler::with_demo(catalog).with_value_limits(max_value, max_key);
    if suffix.is_some() {
        handler = handler.with_protocol_config(
            "us-east-1".into(),
            suffix.clone(),
            Duration::from_secs(24 * 3600),
            5 * 1024 * 1024,
            10_000,
        );
    }

    let listener = TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|e| panic!("bind {listen}: {e}"));
    eprintln!(
        "s3_listen {listen} key=demo region=us-east-1 vhost={} max_value={max_value}",
        suffix.as_deref().unwrap_or("-")
    );

    let cancel = CancellationToken::new();
    loop {
        let (sock, _) = listener.accept().await.expect("accept");
        let handler = handler.clone();
        let cancel = cancel.child_token();
        tokio::spawn(async move {
            handler.serve(sock, cancel).await;
        });
    }
}
