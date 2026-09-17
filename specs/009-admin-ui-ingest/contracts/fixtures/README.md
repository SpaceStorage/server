# Config fixtures for `009` contract tests

Valid starters (complete-product / slice 11 profile):

| File | Purpose |
|------|---------|
| [ingest-kafka.conf](ingest-kafka.conf) | Kafka consumer declaration bootstrap |
| [ingest-syslog.conf](ingest-syslog.conf) | Dedicated syslog listen port 1:1 to `acme/events` |

Invalid (each file one `validate` failure):

| File | Expected code |
|------|----------------|
| `invalid/syslog-on-first-binary.conf` | `UiIngestSlice11Required` |
| `invalid/kafka-on-first-binary.conf` | `UiIngestSlice11Required` |
| `invalid/syslog-missing-target.conf` | `ingest_missing_target` |
| `invalid/syslog-duplicate-port.conf` | `entrypoint_duplicate_address` |
| `invalid/kafka-empty-brokers.conf` | `ingest_kafka_no_brokers` |
