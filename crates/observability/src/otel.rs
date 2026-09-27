//! OTLP/HTTP protobuf push (slice 9).

use crate::filter::filter_namespace;
use crate::otlp_encode::encode_otlp_metrics;
use crate::sample::SeriesSnapshot;
use crate::ObservabilityError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct OtelConfig {
    pub endpoint: String,
    pub interval: Duration,
}

impl OtelConfig {
    pub fn validate(&self) -> Result<(), ObservabilityError> {
        let e = self.endpoint.trim();
        if !(e.starts_with("http://") || e.starts_with("https://")) {
            return Err(ObservabilityError::SinkConfigInvalid {
                detail: "OTLP endpoint must be http(s)".into(),
            });
        }
        Ok(())
    }

    pub fn metrics_url(&self) -> String {
        let base = self.endpoint.trim_end_matches('/');
        format!("{base}/v1/metrics")
    }
}

#[derive(Debug, Default)]
pub struct OtelExporter {
    pub errors: AtomicU64,
    pub pushes: AtomicU64,
    /// When set, push encodes OTLP but stores the body here instead of HTTP (unit tests).
    capture: parking_lot::Mutex<Option<Vec<u8>>>,
}

impl OtelExporter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Capture mode: encode OTLP protobuf without requiring a collector.
    pub fn capture() -> Self {
        Self {
            capture: parking_lot::Mutex::new(Some(Vec::new())),
            ..Self::default()
        }
    }

    pub fn last_captured(&self) -> Option<Vec<u8>> {
        self.capture.lock().clone()
    }

    /// Push same series set as scrape (omit-label included). Never blocks recorders.
    pub async fn push(
        &self,
        cfg: &OtelConfig,
        series: &[SeriesSnapshot],
        namespace_filter: Option<&str>,
        node_id: &str,
    ) -> Result<(), ObservabilityError> {
        cfg.validate()?;
        let filtered: Vec<SeriesSnapshot> = match namespace_filter {
            Some(ns) => filter_namespace(series, ns),
            None => series.to_vec(),
        };
        let body = encode_otlp_metrics(&filtered, node_id);
        let url = cfg.metrics_url();
        debug!(
            %url,
            service.name = "spacestorage",
            service.instance.id = %node_id,
            service.namespace = "spacestorage",
            bytes = body.len(),
            "otel push"
        );

        if self.capture.lock().is_some() {
            *self.capture.lock() = Some(body);
            self.pushes.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        match http_post_protobuf(&url, &body).await {
            Ok(()) => {
                self.pushes.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(e) => {
                self.record_failure(namespace_filter.unwrap_or("global"));
                Err(e)
            }
        }
    }

    pub fn record_failure(&self, stream: &str) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        warn!(stream, "otel export failed");
    }
}

async fn http_post_protobuf(url: &str, body: &[u8]) -> Result<(), ObservabilityError> {
    let parsed = parse_http_url(url)?;
    let tcp = timeout(
        Duration::from_secs(5),
        TcpStream::connect((parsed.host.as_str(), parsed.port)),
    )
    .await
    .map_err(|_| ObservabilityError::SinkConfigInvalid {
        detail: "OTLP connect timeout".into(),
    })?
    .map_err(|e| ObservabilityError::SinkConfigInvalid {
        detail: format!("OTLP connect: {e}"),
    })?;

    let host_header = if (!parsed.tls && parsed.port == 80) || (parsed.tls && parsed.port == 443) {
        parsed.host.clone()
    } else {
        format!("{}:{}", parsed.host, parsed.port)
    };
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/x-protobuf\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        parsed.path,
        host_header,
        body.len()
    );

    if parsed.tls {
        let mut tls = tls_connect(&parsed.host, tcp).await?;
        tls.write_all(req.as_bytes())
            .await
            .map_err(|e| ObservabilityError::SinkConfigInvalid {
                detail: format!("OTLP TLS write headers: {e}"),
            })?;
        tls.write_all(body)
            .await
            .map_err(|e| ObservabilityError::SinkConfigInvalid {
                detail: format!("OTLP TLS write body: {e}"),
            })?;
        let mut resp = Vec::new();
        let _ = timeout(Duration::from_secs(5), tls.read_to_end(&mut resp)).await;
        return check_http_status(&resp);
    }

    let mut stream = tcp;
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| ObservabilityError::SinkConfigInvalid {
            detail: format!("OTLP write headers: {e}"),
        })?;
    stream
        .write_all(body)
        .await
        .map_err(|e| ObservabilityError::SinkConfigInvalid {
            detail: format!("OTLP write body: {e}"),
        })?;

    let mut resp = Vec::new();
    let _ = timeout(Duration::from_secs(5), stream.read_to_end(&mut resp)).await;
    check_http_status(&resp)
}

fn check_http_status(resp: &[u8]) -> Result<(), ObservabilityError> {
    let text = String::from_utf8_lossy(resp);
    let status_ok = text.starts_with("HTTP/1.1 2") || text.starts_with("HTTP/1.0 2");
    if !status_ok {
        return Err(ObservabilityError::SinkConfigInvalid {
            detail: format!(
                "OTLP HTTP status not 2xx: {}",
                text.lines().next().unwrap_or("(empty)")
            ),
        });
    }
    Ok(())
}

async fn tls_connect(
    host: &str,
    tcp: TcpStream,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, ObservabilityError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(cfg));
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| {
        ObservabilityError::SinkConfigInvalid {
            detail: format!("OTLP TLS server name '{host}': {e}"),
        }
    })?;
    timeout(Duration::from_secs(5), connector.connect(server_name, tcp))
        .await
        .map_err(|_| ObservabilityError::SinkConfigInvalid {
            detail: "OTLP TLS handshake timeout".into(),
        })?
        .map_err(|e| ObservabilityError::SinkConfigInvalid {
            detail: format!("OTLP TLS handshake: {e}"),
        })
}

struct ParsedUrl {
    host: String,
    port: u16,
    path: String,
    tls: bool,
}

fn parse_http_url(url: &str) -> Result<ParsedUrl, ObservabilityError> {
    let tls = url.starts_with("https://");
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or_else(|| ObservabilityError::SinkConfigInvalid {
            detail: "OTLP endpoint must be http(s)".into(),
        })?;
    let (hostport, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".into()),
    };
    let (host, port) = if let Some((h, p)) = hostport.split_once(':') {
        (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| ObservabilityError::SinkConfigInvalid {
                    detail: format!("bad OTLP port '{p}'"),
                })?,
        )
    } else {
        (hostport.to_string(), if tls { 443 } else { 80 })
    };
    Ok(ParsedUrl {
        host,
        port,
        path,
        tls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{LabelSet, SampleKind};

    #[tokio::test]
    async fn capture_push_emits_otlp_protobuf() {
        let exp = OtelExporter::capture();
        let mut labels = LabelSet::new();
        let _ = labels.insert("kind", "query");
        let series = [SeriesSnapshot {
            name: "spacestorage_query_total".into(),
            labels,
            kind: SampleKind::Counter,
            help: "q",
            counter: Some(1),
            gauge: None,
            hist_bounds: None,
            hist_counts: None,
            hist_sum: None,
            hist_count: None,
        }];
        exp.push(
            &OtelConfig {
                endpoint: "http://127.0.0.1:4318".into(),
                interval: Duration::from_secs(15),
            },
            &series,
            None,
            "node-uuid",
        )
        .await
        .unwrap();
        let body = exp.last_captured().unwrap();
        assert!(!body.is_empty());
        assert_eq!(body[0], 0x0a);
        assert!(body
            .windows(b"service.name".len())
            .any(|w| w == b"service.name")
            || body.windows(b"spacestorage".len()).any(|w| w == b"spacestorage"));
        assert_eq!(exp.pushes.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn http_post_to_local_listener() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            // Read until Connection: close peer finishes or we have body bytes.
            loop {
                match sock.read(&mut tmp).await {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            // Got headers; if Content-Length satisfied, stop.
                            let text = String::from_utf8_lossy(&buf);
                            if let Some(cl) = text
                                .lines()
                                .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                                .and_then(|l| l.split(':').nth(1))
                                .and_then(|v| v.trim().parse::<usize>().ok())
                            {
                                if let Some(pos) = text.find("\r\n\r\n") {
                                    if buf.len() >= pos + 4 + cl {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let req = String::from_utf8_lossy(&buf);
            assert!(req.contains("POST /v1/metrics"));
            assert!(req.contains("application/x-protobuf"));
            assert!(
                buf.windows(b"spacestorage".len())
                    .any(|w| w == b"spacestorage"),
                "OTLP body should include resource service.name=spacestorage"
            );
            sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        });

        let exp = OtelExporter::new();
        let series = [SeriesSnapshot {
            name: "spacestorage_uptime_seconds".into(),
            labels: LabelSet::new(),
            kind: SampleKind::Gauge,
            help: "up",
            counter: None,
            gauge: Some(1.0),
            hist_bounds: None,
            hist_counts: None,
            hist_sum: None,
            hist_count: None,
        }];
        exp.push(
            &OtelConfig {
                endpoint: format!("http://{addr}"),
                interval: Duration::from_secs(15),
            },
            &series,
            None,
            "n1",
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(exp.pushes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn parse_https_defaults_port_443() {
        let p = parse_http_url("https://otel.example/v1/metrics").unwrap();
        assert!(p.tls);
        assert_eq!(p.host, "otel.example");
        assert_eq!(p.port, 443);
        assert_eq!(p.path, "/v1/metrics");
    }
}
