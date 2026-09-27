//! Pure-Rust Kafka Produce for outbound logs (008; no rdkafka).
//!
//! Builds real ProduceRequest frames via `kafka-protocol` and sends them over TCP.
//! Unit / conformance tests use [`KafkaProducer::memory`] which still encodes a
//! Produce frame but records locally without requiring a broker.

use crate::logs::LogEvent;
use crate::ObservabilityError;
use bytes::{Bytes, BytesMut};
use kafka_protocol::{
    messages::{
        produce_request::{PartitionProduceData, TopicProduceData},
        ApiKey, ProduceRequest, ProduceResponse, RequestHeader, ResponseHeader, TopicName,
    },
    protocol::{
        Decodable as KafkaDecodable, Encodable as KafkaEncodable, HeaderVersion, StrBytes,
    },
    records::{
        Compression, Record, RecordBatchEncoder, RecordEncodeOptions, TimestampType,
        NO_PRODUCER_EPOCH, NO_PRODUCER_ID, NO_SEQUENCE,
    },
};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{debug, warn};

const CLIENT_ID: &str = "spacestorage-observability";
const PRODUCE_API: i16 = 7;

#[derive(Debug, Clone)]
pub struct KafkaSinkConfig {
    pub brokers: Vec<String>,
    pub topic: String,
}

impl KafkaSinkConfig {
    pub fn validate(&self) -> Result<(), ObservabilityError> {
        if self.brokers.is_empty() || self.brokers.iter().all(|b| b.trim().is_empty()) {
            return Err(ObservabilityError::SinkConfigInvalid {
                detail: "kafka brokers empty".into(),
            });
        }
        if self.topic.is_empty() {
            return Err(ObservabilityError::SinkConfigInvalid {
                detail: "kafka topic empty".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ProducedRecord {
    pub topic: String,
    pub key: String,
    pub payload: Vec<u8>,
    /// Length-prefixed ProduceRequest frame that would be sent on the wire.
    pub wire_frame: Vec<u8>,
}

#[derive(Debug)]
enum ProduceMode {
    /// Encode Produce frames and POST to brokers.
    Live,
    /// Encode Produce frames into an in-memory outbox (tests / offline).
    Memory(Mutex<Vec<ProducedRecord>>),
}

#[derive(Debug)]
pub struct KafkaProducer {
    pub sent: AtomicU64,
    pub errors: AtomicU64,
    correlation: AtomicI32,
    mode: ProduceMode,
}

impl Default for KafkaProducer {
    fn default() -> Self {
        Self::new()
    }
}

impl KafkaProducer {
    /// Live TCP Produce against configured brokers.
    pub fn new() -> Self {
        Self {
            sent: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            correlation: AtomicI32::new(1),
            mode: ProduceMode::Live,
        }
    }

    /// Memory outbox: still builds real Produce frames for conformance without a broker.
    pub fn memory() -> Self {
        Self {
            sent: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            correlation: AtomicI32::new(1),
            mode: ProduceMode::Memory(Mutex::new(Vec::new())),
        }
    }

    pub fn memory_outbox(&self) -> Option<Vec<ProducedRecord>> {
        match &self.mode {
            ProduceMode::Memory(m) => Some(m.lock().clone()),
            ProduceMode::Live => None,
        }
    }

    /// At-least-once Produce: JSON payload; key = namespace or `_cluster`.
    pub async fn send(
        &self,
        cfg: &KafkaSinkConfig,
        event: &LogEvent,
    ) -> Result<(), ObservabilityError> {
        cfg.validate()?;
        let key = event.namespace.as_deref().unwrap_or("_cluster");
        let payload = serde_json::json!({
            "time": event.time,
            "channel": event.channel.as_str(),
            "node": event.node,
            "namespace": event.namespace,
            "severity": event.severity,
            "message": event.message,
            "fields": event.fields,
            "audit": event.audit,
        });
        let value = serde_json::to_vec(&payload).map_err(|e| ObservabilityError::SinkConfigInvalid {
            detail: format!("kafka json: {e}"),
        })?;
        let frame = encode_produce_frame(
            &cfg.topic,
            key.as_bytes(),
            &value,
            self.correlation.fetch_add(1, Ordering::Relaxed),
        )
        .map_err(|e| ObservabilityError::SinkConfigInvalid {
            detail: format!("kafka produce encode: {e}"),
        })?;

        match &self.mode {
            ProduceMode::Memory(outbox) => {
                outbox.lock().push(ProducedRecord {
                    topic: cfg.topic.clone(),
                    key: key.to_string(),
                    payload: value,
                    wire_frame: frame,
                });
                debug!(brokers = ?cfg.brokers, topic = %cfg.topic, key, "kafka produce (memory)");
                self.sent.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            ProduceMode::Live => {
                match self.produce_live(cfg, &frame).await {
                    Ok(()) => {
                        self.sent.fetch_add(1, Ordering::Relaxed);
                        Ok(())
                    }
                    Err(e) => {
                        self.errors.fetch_add(1, Ordering::Relaxed);
                        warn!(error = %e, "kafka produce failed");
                        Err(e)
                    }
                }
            }
        }
    }

    async fn produce_live(
        &self,
        cfg: &KafkaSinkConfig,
        frame: &[u8],
    ) -> Result<(), ObservabilityError> {
        let mut last = None;
        for broker in &cfg.brokers {
            match timeout(Duration::from_secs(3), TcpStream::connect(broker.as_str())).await {
                Ok(Ok(mut stream)) => {
                    stream.write_all(frame).await.map_err(|e| {
                        ObservabilityError::SinkConfigInvalid {
                            detail: format!("kafka write: {e}"),
                        }
                    })?;
                    let mut len_buf = [0u8; 4];
                    stream.read_exact(&mut len_buf).await.map_err(|e| {
                        ObservabilityError::SinkConfigInvalid {
                            detail: format!("kafka read len: {e}"),
                        }
                    })?;
                    let msg_size = i32::from_be_bytes(len_buf) as usize;
                    let mut buf = vec![0u8; msg_size];
                    stream.read_exact(&mut buf).await.map_err(|e| {
                        ObservabilityError::SinkConfigInvalid {
                            detail: format!("kafka read body: {e}"),
                        }
                    })?;
                    let mut buf = Bytes::from(buf);
                    let _hdr = <ResponseHeader as KafkaDecodable>::decode(
                        &mut buf,
                        ProduceResponse::header_version(PRODUCE_API),
                    )
                    .map_err(|e| ObservabilityError::SinkConfigInvalid {
                        detail: format!("kafka resp header: {e}"),
                    })?;
                    let resp = <ProduceResponse as KafkaDecodable>::decode(&mut buf, PRODUCE_API)
                        .map_err(|e| ObservabilityError::SinkConfigInvalid {
                            detail: format!("kafka resp body: {e}"),
                        })?;
                    for t in resp.responses {
                        for p in t.partition_responses {
                            if p.error_code != 0 {
                                return Err(ObservabilityError::SinkConfigInvalid {
                                    detail: format!("kafka produce error_code={}", p.error_code),
                                });
                            }
                        }
                    }
                    return Ok(());
                }
                Ok(Err(e)) => last = Some(e.to_string()),
                Err(_) => last = Some("timeout".into()),
            }
        }
        Err(ObservabilityError::SinkConfigInvalid {
            detail: format!(
                "kafka produce unreachable: {}",
                last.unwrap_or_else(|| "no brokers".into())
            ),
        })
    }
}

fn encode_produce_frame(
    topic: &str,
    key: &[u8],
    value: &[u8],
    correlation_id: i32,
) -> Result<Vec<u8>, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let record = Record {
        transactional: false,
        control: false,
        partition_leader_epoch: 0,
        producer_id: NO_PRODUCER_ID,
        producer_epoch: NO_PRODUCER_EPOCH,
        timestamp_type: TimestampType::Creation,
        offset: 0,
        sequence: NO_SEQUENCE,
        timestamp: now,
        key: Some(Bytes::copy_from_slice(key)),
        value: Some(Bytes::copy_from_slice(value)),
        headers: Default::default(),
    };
    let mut encoded = BytesMut::new();
    RecordBatchEncoder::encode_with_custom_compression(
        &mut encoded,
        [&record],
        &RecordEncodeOptions {
            version: 2,
            compression: Compression::None,
        },
        None::<fn(&mut BytesMut, &mut BytesMut, Compression) -> anyhow::Result<()>>,
    )
    .map_err(|e| e.to_string())?;

    let mut req = ProduceRequest::default();
    req.acks = 1;
    req.timeout_ms = 5000;
    req.topic_data = vec![TopicProduceData::default()
        .with_name(TopicName(StrBytes::from_string(topic.to_string())))
        .with_partition_data(vec![PartitionProduceData::default()
            .with_index(0)
            .with_records(Some(encoded.freeze()))])];

    let mut header = RequestHeader::default();
    header.request_api_key = ApiKey::Produce as i16;
    header.request_api_version = PRODUCE_API;
    header.correlation_id = correlation_id;
    header.client_id = Some(StrBytes::from_static_str(CLIENT_ID));

    let mut bytes = BytesMut::new();
    <RequestHeader as KafkaEncodable>::encode(
        &header,
        &mut bytes,
        ProduceRequest::header_version(PRODUCE_API),
    )
    .map_err(|e| e.to_string())?;
    <ProduceRequest as KafkaEncodable>::encode(&req, &mut bytes, PRODUCE_API)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
    out.extend_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::LogChannel;

    #[tokio::test]
    async fn memory_produce_encodes_wire_frame() {
        let p = KafkaProducer::memory();
        p.send(
            &KafkaSinkConfig {
                brokers: vec!["127.0.0.1:9092".into()],
                topic: "logs".into(),
            },
            &LogEvent {
                time: "t".into(),
                channel: LogChannel::Default,
                node: "n1".into(),
                namespace: Some("acme".into()),
                severity: "err".into(),
                message: "hi".into(),
                fields: serde_json::json!({}),
                audit: None,
            },
        )
        .await
        .unwrap();
        let out = p.memory_outbox().unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "acme");
        assert!(out[0].wire_frame.len() > 4);
        let len = i32::from_be_bytes(out[0].wire_frame[0..4].try_into().unwrap()) as usize;
        assert_eq!(len, out[0].wire_frame.len() - 4);
    }
}
