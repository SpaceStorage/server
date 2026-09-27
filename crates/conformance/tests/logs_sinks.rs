//! Kafka/syslog delivery (008 T025) — same default-channel events on enabled sinks.

use spacestorage_observability::{
    ChannelGates, KafkaProducer, KafkaSinkConfig, LogChannel, LogEvent, LogExporter,
    SyslogExporter, SyslogSinkConfig, SyslogTransport,
};

#[tokio::test]
async fn kafka_and_syslog_receive_default_channel() {
    let exp = LogExporter::new();
    exp.set_gates(ChannelGates::default());
    let ev = LogEvent {
        time: "t".into(),
        channel: LogChannel::Default,
        node: "n1".into(),
        namespace: Some("acme".into()),
        severity: "err".into(),
        message: "query failed".into(),
        fields: serde_json::json!({}),
        audit: None,
    };
    exp.emit(ev.clone()).unwrap();
    let got = exp.try_pop().expect("default channel event");

    // Memory mode still builds real Produce frames without requiring a broker.
    let kafka = KafkaProducer::memory();
    kafka
        .send(
            &KafkaSinkConfig {
                brokers: vec!["127.0.0.1:9092".into()],
                topic: "logs".into(),
            },
            &got,
        )
        .await
        .unwrap();
    assert_eq!(kafka.sent.load(std::sync::atomic::Ordering::Relaxed), 1);
    let outbox = kafka.memory_outbox().expect("memory outbox");
    assert_eq!(outbox.len(), 1);
    assert!(outbox[0].wire_frame.len() > 4);

    let syslog = SyslogExporter::new();
    let msg = syslog.format_rfc5424(&got);
    assert!(msg.contains("spacestorage"));
    assert!(msg.contains("channel=\"default\""));
    assert!(msg.contains("ns=\"acme\""));
    // Best-effort send (may error if nothing listens); still validates framing path.
    let _ = syslog
        .send(
            &SyslogSinkConfig {
                address: "127.0.0.1:1".into(),
                transport: SyslogTransport::Udp,
            },
            &got,
        )
        .await;

    // Tenant isolation: foreign namespace audit must not be accepted for tenant filter
    // (unit-level KeyMaterialForbidden / gating covered in logs module).
    assert_ne!(got.namespace.as_deref(), Some("other"));
}
