//! Kafka consumer group: Fetch → decode → append → durable ack → OffsetCommit.

use crate::declaration::KafkaIngest;
use crate::format::{map_kafka_payload, KafkaSidecar, PayloadFormat};
use crate::kafka_wire::LiveKafkaClient;
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

/// In-process pure-Rust fake broker for ALO unit / conformance tests (SC-004).
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

/// Backend used by [`KafkaConsumer`]: fake (tests) or live broker I/O.
pub enum KafkaBrokerBackend {
    Fake(Arc<FakeKafkaBroker>),
    Live(Arc<LiveKafkaClient>),
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
    pub broker: KafkaBrokerBackend,
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
            broker: KafkaBrokerBackend::Fake(broker),
            buffer: DecodeBuffer::new(32 * 1024 * 1024),
            append,
            runtime: None,
            skip_commit_once: Mutex::new(false),
            stored: Mutex::new(Vec::new()),
        }
    }

    /// Live broker Fetch / OffsetCommit path (production ingest).
    pub fn with_live(decl: KafkaIngest, live: Arc<LiveKafkaClient>, append: AppendFn) -> Self {
        Self {
            decl,
            broker: KafkaBrokerBackend::Live(live),
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

    /// Sync poll for FakeKafkaBroker (ALO unit tests).
    pub fn poll_one(&self) -> Result<bool, String> {
        match &self.broker {
            KafkaBrokerBackend::Fake(b) => {
                let Some(msg) = b.fetch() else {
                    return Ok(false);
                };
                self.handle_message(msg, |m| b.produce(m), |p, o| b.commit(p, o))
            }
            KafkaBrokerBackend::Live(_) => Err(
                "poll_one is FakeKafka-only; use poll_one_async for live brokers".into(),
            ),
        }
    }

    /// Async poll: Fake or live broker.
    pub async fn poll_one_async(&self) -> Result<bool, String> {
        match &self.broker {
            KafkaBrokerBackend::Fake(b) => {
                let Some(msg) = b.fetch() else {
                    return Ok(false);
                };
                self.handle_message(msg, |m| b.produce(m), |p, o| b.commit(p, o))
            }
            KafkaBrokerBackend::Live(live) => {
                let Some(msg) = live.fetch_one().await? else {
                    return Ok(false);
                };
                let partition = msg.partition;
                let offset = msg.offset;
                let live_requeue = Arc::clone(live);
                let pending_commit = Mutex::new(None::<(i32, i64)>);
                let outcome = self.handle_message(
                    msg,
                    |m| live_requeue.requeue(m),
                    |p, o| {
                        *pending_commit.lock() = Some((p, o));
                    },
                )?;
                if let Some((p, o)) = pending_commit.into_inner() {
                    live.commit(p, o).await?;
                    let _ = (partition, offset);
                }
                Ok(outcome)
            }
        }
    }

    fn handle_message(
        &self,
        msg: KafkaMessage,
        requeue: impl Fn(KafkaMessage),
        commit: impl Fn(i32, i64),
    ) -> Result<bool, String> {
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
            requeue(msg);
            return Ok(false);
        }
        let size = msg.value.len();
        let partition = msg.partition;
        let offset = msg.offset;
        let sidecar = KafkaSidecar {
            key: msg
                .key
                .as_ref()
                .map(|k| String::from_utf8_lossy(k).into_owned()),
            topic: msg.topic.clone(),
            partition,
            offset,
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
                Ok(false)
            }
            Ok(rec) => {
                let ack = (self.append)(rec.clone());
                if !ack.durable {
                    requeue(msg);
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
                    return Ok(false);
                }
                commit(partition, offset);
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
