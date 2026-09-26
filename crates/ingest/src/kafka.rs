//! Kafka consumer group: Fetch → decode → append → durable ack → OffsetCommit.

use crate::declaration::KafkaIngest;
use crate::format::{map_kafka_payload, KafkaSidecar, PayloadFormat};
use crate::record::LogRecord;
use crate::IngestRuntime;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct KafkaMessage {
    pub key: Option<Vec<u8>>,
    pub value: Vec<u8>,
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub broker_ts: Option<String>,
}

/// In-process pure-Rust fake broker for conformance (SC-004).
#[derive(Debug, Default)]
pub struct FakeKafkaBroker {
    pub messages: Mutex<VecDeque<KafkaMessage>>,
    /// Committed offsets per partition.
    pub committed: Mutex<std::collections::BTreeMap<i32, i64>>,
    pub fetch_count: AtomicU64,
}

impl FakeKafkaBroker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn produce(&self, msg: KafkaMessage) {
        self.messages.lock().push_back(msg);
    }

    pub fn fetch(&self) -> Option<KafkaMessage> {
        self.fetch_count.fetch_add(1, Ordering::SeqCst);
        self.messages.lock().pop_front()
    }

    pub fn commit(&self, partition: i32, offset: i64) {
        self.committed.lock().insert(partition, offset);
    }

    pub fn committed_offset(&self, partition: i32) -> Option<i64> {
        self.committed.lock().get(&partition).copied()
    }
}

/// Decode buffer with reject policy (never wait on worker).
#[derive(Debug)]
pub struct DecodeBuffer {
    capacity: usize,
    used: AtomicU64,
}

impl DecodeBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            used: AtomicU64::new(0),
        }
    }

    pub fn try_enqueue(&self, bytes: usize) -> bool {
        let cur = self.used.load(Ordering::SeqCst) as usize;
        if cur + bytes > self.capacity {
            return false;
        }
        self.used.fetch_add(bytes as u64, Ordering::SeqCst);
        true
    }

    pub fn release(&self, bytes: usize) {
        self.used.fetch_sub(bytes as u64, Ordering::SeqCst);
    }
}

pub struct AppendAck {
    pub durable: bool,
}

/// Sink for durable log_stream.append (injected by node / tests).
pub type AppendFn = Arc<dyn Fn(LogRecord) -> AppendAck + Send + Sync>;

pub struct KafkaConsumer {
    pub decl: KafkaIngest,
    pub broker: Arc<FakeKafkaBroker>,
    pub buffer: DecodeBuffer,
    pub append: AppendFn,
    pub runtime: Option<Arc<IngestRuntime>>,
    /// When true, simulate crash after durable ack before OffsetCommit.
    pub skip_commit_once: Mutex<bool>,
    pub stored: Mutex<Vec<LogRecord>>,
}

impl KafkaConsumer {
    pub fn new(decl: KafkaIngest, broker: Arc<FakeKafkaBroker>, append: AppendFn) -> Self {
        Self {
            decl,
            broker,
            buffer: DecodeBuffer::new(32 * 1024 * 1024),
            append,
            runtime: None,
            skip_commit_once: Mutex::new(false),
            stored: Mutex::new(Vec::new()),
        }
    }

    pub fn with_buffer_capacity(mut self, cap: usize) -> Self {
        self.buffer = DecodeBuffer::new(cap);
        self
    }

    pub fn with_runtime(mut self, rt: Arc<IngestRuntime>) -> Self {
        self.runtime = Some(rt);
        self
    }

    /// Process one fetched message. Returns Ok(true) if committed.
    pub fn poll_one(&self) -> Result<bool, String> {
        let Some(msg) = self.broker.fetch() else {
            return Ok(false);
        };
        if !self.buffer.try_enqueue(msg.value.len()) {
            if let Some(rt) = &self.runtime {
                rt.bump_dropped("kafka", "buffer_full");
                rt.bump_record(
                    "kafka",
                    &self.decl.namespace,
                    &self.decl.container,
                    format_label(self.decl.format),
                    "dropped",
                );
            }
            // Do not commit; re-queue for retry (at-least-once).
            self.broker.produce(msg);
            return Ok(false);
        }
        let size = msg.value.len();
        let sidecar = KafkaSidecar {
            key: msg
                .key
                .as_ref()
                .map(|k| String::from_utf8_lossy(k).into_owned()),
            topic: msg.topic.clone(),
            partition: msg.partition,
            offset: msg.offset,
            broker_ts: msg.broker_ts.clone(),
        };
        let mapped = map_kafka_payload(self.decl.format, &msg.value, &sidecar);
        self.buffer.release(size);
        match mapped {
            Err(e) => {
                if let Some(rt) = &self.runtime {
                    rt.bump_parse_error(
                        "kafka",
                        &self.decl.namespace,
                        &self.decl.container,
                        e.reason(),
                    );
                    rt.bump_record(
                        "kafka",
                        &self.decl.namespace,
                        &self.decl.container,
                        format_label(self.decl.format),
                        "parse_error",
                    );
                }
                // Skip: no commit for this offset; partition continues.
                Ok(false)
            }
            Ok(rec) => {
                let ack = (self.append)(rec.clone());
                if !ack.durable {
                    // Do not commit; retry later.
                    self.broker.produce(msg);
                    return Ok(false);
                }
                self.stored.lock().push(rec);
                if let Some(rt) = &self.runtime {
                    rt.bump_record(
                        "kafka",
                        &self.decl.namespace,
                        &self.decl.container,
                        format_label(self.decl.format),
                        "ok",
                    );
                }
                let mut skip = self.skip_commit_once.lock();
                if *skip {
                    *skip = false;
                    // Crash between durable ack and OffsetCommit — duplicates MAY appear.
                    return Ok(false);
                }
                self.broker.commit(msg.partition, msg.offset);
                if let Some(rt) = &self.runtime {
                    rt.bump_offset_commit(&self.decl.namespace, &self.decl.container, "ok");
                }
                Ok(true)
            }
        }
    }
}

fn format_label(f: PayloadFormat) -> &'static str {
    match f {
        PayloadFormat::Raw => "raw",
        PayloadFormat::Json => "json",
    }
}

// Keep kafka-protocol linked (pure-Rust framing; FakeKafkaBroker covers conformance).
#[allow(dead_code)]
fn _kafka_protocol_touch() {
    let _ = std::mem::size_of::<kafka_protocol::messages::ResponseHeader>();
}
