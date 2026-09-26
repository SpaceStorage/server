//! RFC 5424 syslog UDP/TCP (octet-counted on TCP) for outbound logs.

use crate::logs::{LogChannel, LogEvent};
use crate::ObservabilityError;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyslogTransport {
    Udp,
    Tcp,
}

#[derive(Debug, Clone)]
pub struct SyslogSinkConfig {
    pub address: String,
    pub transport: SyslogTransport,
}

impl SyslogSinkConfig {
    pub fn validate(&self) -> Result<(), ObservabilityError> {
        if self.address.is_empty() || !self.address.contains(':') {
            return Err(ObservabilityError::SinkConfigInvalid {
                detail: "syslog address unparseable".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct SyslogExporter {
    pub sent: AtomicU64,
    pub errors: AtomicU64,
}

impl SyslogExporter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn format_rfc5424(&self, event: &LogEvent) -> String {
        // facility local0 = 16; severity from channel
        let sev = match event.channel {
            LogChannel::Default if event.severity == "err" => 3, // err
            LogChannel::Default => 6,                          // info
            LogChannel::SlowQuery | LogChannel::Audit => 5,    // notice
        };
        let pri = 16 * 8 + sev;
        let ns = event.namespace.as_deref().unwrap_or("-");
        let hostname = "-";
        format!(
            "<{pri}>1 {time} {hostname} spacestorage {pid} - [ss@32473 ns=\"{ns}\" channel=\"{ch}\" node=\"{node}\"] {msg}",
            time = event.time,
            pid = std::process::id(),
            ch = event.channel.as_str(),
            node = event.node,
            msg = event.message,
        )
    }

    pub async fn send(
        &self,
        cfg: &SyslogSinkConfig,
        event: &LogEvent,
    ) -> Result<(), ObservabilityError> {
        cfg.validate()?;
        let msg = self.format_rfc5424(event);
        match cfg.transport {
            SyslogTransport::Udp => {
                // Best-effort; failures count as errors without blocking recorders.
                match UdpSocket::bind("0.0.0.0:0").await {
                    Ok(sock) => {
                        if sock.send_to(msg.as_bytes(), &cfg.address).await.is_err() {
                            self.errors.fetch_add(1, Ordering::Relaxed);
                        } else {
                            self.sent.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        self.errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            SyslogTransport::Tcp => {
                match TcpStream::connect(&cfg.address).await {
                    Ok(mut stream) => {
                        // Octet-counted framing
                        let frame = format!("{} {}", msg.len(), msg);
                        if stream.write_all(frame.as_bytes()).await.is_err() {
                            self.errors.fetch_add(1, Ordering::Relaxed);
                        } else {
                            self.sent.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        debug!(addr = %cfg.address, "syslog tcp connect failed");
                        self.errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        Ok(())
    }
}
