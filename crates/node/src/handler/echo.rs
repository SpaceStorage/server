use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use super::Handler;

pub struct EchoHandler;

#[async_trait]
impl Handler for EchoHandler {
    fn name(&self) -> &str {
        "echo"
    }

    async fn serve(&self, mut stream: TcpStream, cancel: CancellationToken) {
        let mut buf = vec![0u8; 4096];
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                r = stream.read(&mut buf) => {
                    match r {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}
