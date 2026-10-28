//! Syslog protocol adapter (009) — 1:1 entrypoint → container via `005` append.

use crate::Node;
use crate::handler::{ClientStream, Handler};
use crate::ingest_append;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio_util::sync::CancellationToken;
use tracing::debug;

pub struct SyslogPortHandler {
    pub node: Arc<Node>,
}

#[async_trait]
impl Handler for SyslogPortHandler {
    fn name(&self) -> &str {
        "syslog"
    }

    fn kind(&self) -> &str {
        "ingest"
    }

    fn owner(&self) -> &str {
        "09-admin-ui-ingest"
    }

    async fn serve(&self, stream: ClientStream, cancel: CancellationToken) {
        self.serve_named(stream, cancel, "").await
    }

    async fn serve_named(
        &self,
        mut stream: ClientStream,
        cancel: CancellationToken,
        entrypoint: &str,
    ) {
        if !self.node.ingest.slice11_enabled {
            return;
        }
        if self.node.is_draining() {
            return;
        }
        let cfg = self.node.config.load();
        let ep = cfg
            .entrypoints
            .iter()
            .find(|e| e.handler == "syslog" && (entrypoint.is_empty() || e.name == entrypoint));
        let Some(ep) = ep else {
            return;
        };
        // TLS is TCP-only (entrypoint acceptor); refuse if somehow mis-bound.
        let Some(ing) = &ep.ingest else {
            return;
        };
        let ns = ing.namespace.clone();
        let container = ing.container.clone();
        let catalog = Arc::clone(&self.node.catalog);
        let store_fn: spacestorage_ingest::syslog::StoreFn = Arc::new(move |rec| {
            let _ = ingest_append::append_log_record(&catalog, &ns, &container, rec);
        });
        let Ok(handler) = spacestorage_ingest::SyslogHandler::new(
            spacestorage_ingest::SyslogIngestBind {
                entrypoint: ep.name.clone(),
                namespace: ing.namespace.clone(),
                container: ing.container.clone(),
                type_name: ing.type_name.clone(),
            },
            store_fn,
        ) else {
            return;
        };
        let recv_gate = self.node.buffers.gate("ingest.syslog.recv");
        let handler = Arc::new(
            handler
                .with_buffer_gate(recv_gate)
                .with_runtime(Arc::clone(&self.node.ingest)),
        );
        self.node.ingest.register_syslog(Arc::clone(&handler));
        if self.node.is_draining() {
            handler.begin_drain();
            return;
        }

        // Octet-counted RFC 5424 TCP framing, else newline-delimited.
        let mut reader = BufReader::new(&mut stream);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                r = read_syslog_frame(&mut reader) => {
                    match r {
                        Ok(Some(line)) => {
                            match handler.ingest_line(&line) {
                                spacestorage_ingest::IngestOutcome::BufferFull => break,
                                _ => {}
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            debug!(error=%e, "syslog read error");
                            break;
                        }
                    }
                }
            }
        }
    }
}

async fn read_syslog_frame<R: AsyncReadExt + Unpin>(
    reader: &mut BufReader<R>,
) -> std::io::Result<Option<String>> {
    let mut peek = [0u8; 1];
    let n = reader.read(&mut peek).await?;
    if n == 0 {
        return Ok(None);
    }
    if peek[0].is_ascii_digit() {
        let mut digits = vec![peek[0]];
        loop {
            let mut b = [0u8; 1];
            let n = reader.read(&mut b).await?;
            if n == 0 {
                break;
            }
            if b[0] == b' ' {
                break;
            }
            if b[0].is_ascii_digit() {
                digits.push(b[0]);
            } else {
                // Not octet-counted — treat as start of a line.
                let mut rest = String::new();
                reader.read_line(&mut rest).await?;
                let mut line = String::from_utf8_lossy(&digits).into_owned();
                line.push(b[0] as char);
                line.push_str(&rest);
                return Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()));
            }
        }
        let len: usize = String::from_utf8_lossy(&digits).parse().unwrap_or(0);
        if len == 0 || len > 64 * 1024 {
            return Ok(None);
        }
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).await?;
        Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
    } else {
        let mut line = String::new();
        line.push(peek[0] as char);
        reader.read_line(&mut line).await?;
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
    }
}

/// UDP datagram loop for plaintext syslog entrypoints (TLS remains TCP-only).
pub async fn serve_udp(
    node: Arc<Node>,
    bind: std::net::SocketAddr,
    ep_name: String,
    cancel: CancellationToken,
) {
    if !node.ingest.slice11_enabled {
        return;
    }
    let sock = match tokio::net::UdpSocket::bind(bind).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(%bind, error=%e, "syslog udp bind failed");
            return;
        }
    };
    tracing::info!(entrypoint=%ep_name, %bind, "syslog udp bound");
    let cfg = node.config.load();
    let Some(ep) = cfg.entrypoints.iter().find(|e| e.name == ep_name) else {
        return;
    };
    let Some(ing) = &ep.ingest else {
        return;
    };
    let ns = ing.namespace.clone();
    let container = ing.container.clone();
    let catalog = Arc::clone(&node.catalog);
    let store_fn: spacestorage_ingest::syslog::StoreFn = Arc::new(move |rec| {
        let _ = ingest_append::append_log_record(&catalog, &ns, &container, rec);
    });
    let Ok(handler) = spacestorage_ingest::SyslogHandler::new(
        spacestorage_ingest::SyslogIngestBind {
            entrypoint: ep.name.clone(),
            namespace: ing.namespace.clone(),
            container: ing.container.clone(),
            type_name: ing.type_name.clone(),
        },
        store_fn,
    ) else {
        return;
    };
    let recv_gate = node.buffers.gate("ingest.syslog.recv");
    let handler = Arc::new(
        handler
            .with_buffer_gate(recv_gate)
            .with_runtime(Arc::clone(&node.ingest)),
    );
    node.ingest.register_syslog(Arc::clone(&handler));
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            r = sock.recv_from(&mut buf) => {
                if node.is_draining() || !handler.accepting() {
                    continue;
                }
                match r {
                    Ok((n, _)) => {
                        let msg = String::from_utf8_lossy(&buf[..n]);
                        for line in msg.split('\n') {
                            let line = line.trim();
                            if !line.is_empty() {
                                // UDP: drop on buffer_full (reject); keep listening.
                                let _ = handler.ingest_line(line);
                            }
                        }
                    }
                    Err(e) => {
                        debug!(error=%e, "syslog udp recv error");
                        break;
                    }
                }
            }
        }
    }
}
