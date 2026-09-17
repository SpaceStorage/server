# Contract: Kafka consumer ingest

**Feature**: `009-admin-ui-ingest` | `crates/ingest/src/kafka.rs`

Not a listen entrypoint (clarify Q2). Complements `008` **produce** (export). This crate **fetches**.

## Group

- `group.id` = declaration `group` (required).
- Members: every node in `ready` (constitution VI).
- Protocol: consumer-group partition assignment. Exactly the assigned partitions are fetched on that member.

## Delivery

1. Fetch records.
2. Decode per [log-record.md](log-record.md) (`raw` default, `json` if declared).
3. Parse failure: increment parse errors, **do not** OffsetCommit that offset, **continue** the partition (skip). Default skip; no dead-letter container in this feature (spec assumption).
4. Success: `log_stream.append` through `005` as application `spacestorage-ingest`.
5. Wait for a **counted durable ack** (`013`/`004` write quorum / durable filter for the container).
6. **Then** OffsetCommit that record (or a contiguous prefix).

Crash between 5 and 6: duplicates MAY appear; previously acked messages are not lost (US3.5, SC-004).

Buffer `ingest.kafka.decode` full: do not enqueue; do not commit; retry fetch; `spacestorage_ingest_dropped_total{reason="buffer_full"}`. Never block a worker (`wait` forbidden).

## TLS

Broker TLS uses declaration `tls` file refs or `plaintext;` — same omit-is-error rule as `001`. Certificates never inlined.

## Exactly-once

MUST NOT be claimed (FR-009). No Kafka transactions required.
