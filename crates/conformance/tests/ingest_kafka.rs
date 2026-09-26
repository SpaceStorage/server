//! Kafka ingest ALO conformance (009 US3) — feature `complete-product`.

#![cfg(feature = "complete-product")]

use spacestorage_ingest::declaration::{IngestAuthz, KafkaIngest};
use spacestorage_ingest::format::PayloadFormat;
use spacestorage_ingest::kafka::{AppendAck, FakeKafkaBroker, KafkaConsumer, KafkaMessage};
use spacestorage_ingest::IngestRuntime;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;

fn decl() -> KafkaIngest {
    KafkaIngest {
        id: Uuid::now_v7(),
        name: "acme-events".into(),
        namespace: "acme".into(),
        container: "events".into(),
        type_name: "log_stream".into(),
        brokers: vec!["127.0.0.1:9092".into()],
        topic: "logs".into(),
        group: "g1".into(),
        format: PayloadFormat::Raw,
        plaintext: true,
        created_by: None,
        state: "running".into(),
    }
}

#[test]
fn write_only_declare_refused() {
    let store = spacestorage_ingest::KafkaIngestStore::new(true);
    let authz = IngestAuthz::write_only_user("acme", "events");
    let err = store.add(decl(), &authz).unwrap_err();
    assert_eq!(err.code(), "ingest_write_only");
}

#[test]
fn durable_ack_then_offset_commit() {
    let broker = Arc::new(FakeKafkaBroker::new());
    let rt = Arc::new(IngestRuntime::new(true));
    let consumer = KafkaConsumer::new(
        decl(),
        Arc::clone(&broker),
        Arc::new(|_| AppendAck { durable: true }),
    )
    .with_runtime(rt);
    broker.produce(KafkaMessage {
        key: Some(b"k".to_vec()),
        value: b"hello".to_vec(),
        topic: "logs".into(),
        partition: 0,
        offset: 7,
        broker_ts: Some("t".into()),
    });
    assert!(consumer.poll_one().unwrap());
    assert_eq!(broker.committed_offset(0), Some(7));
    assert_eq!(consumer.stored.lock().len(), 1);
    assert_eq!(consumer.stored.lock()[0].message, "hello");
}

#[test]
fn crash_after_ack_before_commit_allows_duplicates() {
    let broker = Arc::new(FakeKafkaBroker::new());
    let seen = Arc::new(AtomicBool::new(false));
    let seen2 = Arc::clone(&seen);
    let consumer = KafkaConsumer::new(
        decl(),
        Arc::clone(&broker),
        Arc::new(move |_| {
            seen2.store(true, Ordering::SeqCst);
            AppendAck { durable: true }
        }),
    );
    *consumer.skip_commit_once.lock() = true;
    broker.produce(KafkaMessage {
        key: None,
        value: b"once".to_vec(),
        topic: "logs".into(),
        partition: 0,
        offset: 1,
        broker_ts: None,
    });
    assert!(!consumer.poll_one().unwrap()); // durable stored, no commit
    assert!(seen.load(Ordering::SeqCst));
    assert!(broker.committed_offset(0).is_none());
    assert_eq!(consumer.stored.lock().len(), 1);
    // Re-deliver same offset (ALO): consume again with commit.
    broker.produce(KafkaMessage {
        key: None,
        value: b"once".to_vec(),
        topic: "logs".into(),
        partition: 0,
        offset: 1,
        broker_ts: None,
    });
    assert!(consumer.poll_one().unwrap());
    assert_eq!(consumer.stored.lock().len(), 2); // duplicate ok
    assert_eq!(broker.committed_offset(0), Some(1));
}

#[test]
fn namespace_admin_with_write_accepted() {
    let store = spacestorage_ingest::KafkaIngestStore::new(true);
    let authz = IngestAuthz::namespace_admin_with_write("acme", "events");
    assert!(store.add(decl(), &authz).is_ok());
}
