pub mod tls;

use crate::handler::{ClientStream, Handler};
use crate::Node;
use spacestorage_config::model::Transport;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub async fn bind_all(node: Arc<Node>) -> Result<Vec<tokio::task::JoinHandle<()>>, String> {
    let cfg = node.config.load();
    let mut listeners = Vec::new();
    let mut bound = Vec::new();

    if cfg.disable_admin {
        info!(
            handler = "admin",
            "admin handler disabled by administrator choice (disable admin;); no listener opened"
        );
    }
    if cfg.disable_admin_http {
        info!(
            handler = "admin-http",
            "admin-http handler disabled by administrator choice (disable admin-http;); no listener opened"
        );
    }

    for ep in &cfg.entrypoints {
        if ep.handler == "admin" && cfg.disable_admin {
            continue;
        }
        if ep.handler == "admin-http" && cfg.disable_admin_http {
            continue;
        }
        let addr: SocketAddr = format!("{}:{}", ep.address, ep.port)
            .parse()
            .map_err(|e| format!("bad address for {}: {e}", ep.name))?;

        let tls_acceptor = if ep.transport == Transport::Tls {
            let tls = ep.tls.as_ref().ok_or_else(|| {
                format!(
                    "entrypoint '{}': tls transport requires tls {{ certificate; key; }}",
                    ep.name
                )
            })?;
            Some(tls::load_acceptor(&tls.certificate, &tls.key).await?)
        } else {
            None
        };

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("bind_address_in_use{{ep={}, addr={addr}}}: {e}", ep.name))?;
        info!(entrypoint=%ep.name, %addr, handler=%ep.handler, tls=%tls_acceptor.is_some(), "entrypoint bound");
        bound.push((
            ep.name.clone(),
            ep.handler.clone(),
            listener,
            tls_acceptor,
            ep.tls.as_ref().map(|t| t.certificate.clone()),
        ));
    }

    for (name, handler_name, listener, tls_acceptor, cert_path) in bound {
        let node = node.clone();
        let accept_cancel = node.cancel.clone();
        let handler = node
            .handlers
            .read()
            .unwrap()
            .get(&handler_name)
            .ok_or_else(|| format!("handler '{handler_name}' not registered for {name}"))?;
        let h = tokio::spawn(accept_loop(
            name,
            handler,
            listener,
            accept_cancel,
            node.clone(),
            tls_acceptor,
            cert_path,
        ));
        listeners.push(h);
    }
    Ok(listeners)
}

async fn accept_loop(
    name: String,
    handler: Arc<dyn Handler>,
    listener: TcpListener,
    accept_cancel: CancellationToken,
    node: Arc<Node>,
    tls_acceptor: Option<tokio_rustls::TlsAcceptor>,
    cert_path: Option<String>,
) {
    // Hourly cert expiry probe for TLS entrypoints (FR-028 running marker).
    // PEM read + DER parse run on a blocking pool (Constitution II / T084).
    if let Some(path) = cert_path.clone() {
        let node_exp = node.clone();
        let ep_name = name.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            interval.tick().await;
            loop {
                interval.tick().await;
                if node_exp.cancel.is_cancelled() {
                    break;
                }
                let path = path.clone();
                let expired =
                    match tokio::task::spawn_blocking(move || tls::leaf_expired(&path)).await {
                        Ok(v) => v,
                        Err(e) => {
                            warn!(error=%e, "cert expiry probe join failed");
                            continue;
                        }
                    };
                if expired {
                    node_exp.effective.mark_cert_expired(&ep_name);
                }
            }
        });
    }

    let is_admin = handler.name() == "admin" || handler.name() == "admin-http";

    loop {
        tokio::select! {
            _ = accept_cancel.cancelled() => break,
            acc = listener.accept() => {
                match acc {
                    Ok((stream, peer)) => {
                        // During drain: refuse tenant accepts only; admin stays up (T088 / FR-015).
                        if node.is_draining() && !is_admin {
                            drop(stream);
                            continue;
                        }
                        let force_cancel = node.force_cancel.clone();
                        let handler = handler.clone();
                        let tls_acceptor = tls_acceptor.clone();
                        let stats = node.stats.clone();
                        let ep_name = name.clone();
                        node.tasks.spawn(async move {
                            stats.inc_inflight();
                            let result = async {
                                let client: ClientStream = if let Some(acceptor) = tls_acceptor {
                                    match acceptor.accept(stream).await {
                                        Ok(tls) => Box::new(tls),
                                        Err(e) => {
                                            // No plaintext fallback (FR-027).
                                            warn!(entrypoint=%ep_name, error=%e, "tls handshake failed");
                                            return;
                                        }
                                    }
                                } else {
                                    Box::new(stream)
                                };
                                handler.serve(client, force_cancel).await;
                            }
                            .await;
                            let _ = result;
                            stats.dec_inflight();
                        });
                        let _ = peer;
                    }
                    Err(e) => {
                        error!(entrypoint=%name, error=%e, "accept failed");
                    }
                }
            }
        }
    }
}
