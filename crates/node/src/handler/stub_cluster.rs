use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use super::Handler;

/// Placeholder listener for cluster/protocol ports until those features land.
pub struct ClusterPortHandler {
    pub name: &'static str,
}

#[async_trait]
impl Handler for ClusterPortHandler {
    fn name(&self) -> &str {
        self.name
    }

    fn kind(&self) -> &str {
        "cluster"
    }

    async fn serve(&self, mut stream: TcpStream, cancel: CancellationToken) {
        let mut buf = [0u8; 1024];
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                r = stream.read(&mut buf) => {
                    match r {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
            }
        }
    }
}
