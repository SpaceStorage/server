//! Syslog protocol adapter (009) — 1:1 entrypoint → container.

use crate::handler::{ClientStream, Handler};
use crate::Node;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

    async fn serve(&self, mut stream: ClientStream, cancel: CancellationToken) {
        if !self.node.ingest.slice11_enabled {
            return;
        }
        let cfg = self.node.config.load();
        // Match peer local port to entrypoint; write only to declared ingest target.
        let bind = cfg.entrypoints.iter().find(|e| e.handler == "syslog");
        let Some(ep) = bind else {
            return;
        };
        let Some(ing) = &ep.ingest else {
            return;
        };
        let Ok(handler) = spacestorage_ingest::SyslogHandler::new(
            spacestorage_ingest::SyslogIngestBind {
                entrypoint: ep.name.clone(),
                namespace: ing.namespace.clone(),
                container: ing.container.clone(),
                type_name: ing.type_name.clone(),
            },
            Arc::new(|_| {}),
        ) else {
            return;
        };
        let handler = handler.with_runtime(Arc::clone(&self.node.ingest));
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                r = stream.read(&mut buf) => {
                    match r {
                        Ok(0) => break,
                        Ok(n) => {
                            for line in std::str::from_utf8(&buf[..n]).unwrap_or("").split('\n') {
                                let line = line.trim();
                                if !line.is_empty() {
                                    let _ = handler.ingest_line(line);
                                }
                            }
                            let _ = stream.write_all(b"").await;
                        }
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
