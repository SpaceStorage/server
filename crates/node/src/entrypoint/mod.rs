use crate::handler::Handler;
use crate::Node;
use spacestorage_config::model::Transport;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub async fn bind_all(node: Arc<Node>) -> Result<Vec<tokio::task::JoinHandle<()>>, String> {
    let cfg = node.config.load();
    let mut listeners = Vec::new();
    let mut bound = Vec::new();

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
        if ep.transport == Transport::Tls {
            // TLS acceptor lands with cert files; for MVP plaintext-only first-binary configs.
            return Err(format!(
                "entrypoint '{}': TLS not yet wired in this binary slice (use plaintext; for lab)",
                ep.name
            ));
        }
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("bind_address_in_use{{ep={}, addr={addr}}}: {e}", ep.name))?;
        info!(entrypoint=%ep.name, %addr, handler=%ep.handler, "entrypoint bound");
        bound.push((ep.name.clone(), ep.handler.clone(), listener));
    }

    for (name, handler_name, listener) in bound {
        let node = node.clone();
        let cancel = node.cancel.clone();
        let handler = node
            .handlers
            .read()
            .unwrap()
            .get(&handler_name)
            .ok_or_else(|| format!("handler '{handler_name}' not registered for {name}"))?;
        let h = tokio::spawn(accept_loop(name, handler, listener, cancel, node.clone()));
        listeners.push(h);
    }
    Ok(listeners)
}

async fn accept_loop(
    name: String,
    handler: Arc<dyn Handler>,
    listener: TcpListener,
    cancel: CancellationToken,
    node: Arc<Node>,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            acc = listener.accept() => {
                match acc {
                    Ok((stream, peer)) => {
                        if node.is_draining() && handler.name() != "admin" && handler.name() != "admin-http" {
                            // refuse new tenant connections during drain
                            drop(stream);
                            continue;
                        }
                        let cancel = cancel.clone();
                        let handler = handler.clone();
                        tokio::spawn(async move {
                            handler.serve(stream, cancel).await;
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

pub mod tls {
    // rustls acceptor placeholder for US2; first-binary lab configs use plaintext.
}
