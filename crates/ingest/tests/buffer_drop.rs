//! Buffer overflow reject — drop + metric, no worker wait.

use spacestorage_ingest::declaration::KafkaIngest;
use spacestorage_ingest::format::PayloadFormat;
use spacestorage_ingest::{
    AppendAck, DecodeBuffer, FakeKafkaBroker, IngestRuntime, KafkaConsumer, KafkaMessage,
};
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn decode_buffer_rejects_without_wait() {
    let buf = DecodeBuffer::new(8);
    assert!(buf.try_enqueue(4));
    assert!(!buf.try_enqueue(8)); // would exceed — reject
    buf.release(4);
    assert!(buf.try_enqueue(8));
}

#[test]
fn kafka_buffer_full_drops_and_does_not_commit() {
    let broker = Arc::new(FakeKafkaBroker::new());
    let decl = KafkaIngest {
        id: Uuid::now_v7(),
        name: "t".into(),
        namespace: "acme".into(),
        container: "events".into(),
        type_name: "log_stream".into(),
        brokers: vec!["127.0.0.1:9092".into()],
        topic: "logs".into(),
        group: "g".into(),
        format: PayloadFormat::Raw,
        plaintext: true,
        created_by: None,
        state: "running".into(),
    };
    let rt = Arc::new(IngestRuntime::new(true));
    let consumer = KafkaConsumer::new(
        decl,
        Arc::clone(&broker),
        Arc::new(|_| AppendAck { durable: true }),
    )
    .with_buffer_capacity(4)
    .with_runtime(Arc::clone(&rt));

    broker.produce(KafkaMessage {
        key: None,
        value: b"too-large-payload".to_vec(),
        topic: "logs".into(),
        partition: 0,
        offset: 1,
        broker_ts: None,
    });
    let _ = consumer.poll_one();
    assert!(broker.committed_offset(0).is_none());
    assert!(!broker.messages.lock().is_empty());
}
