# Contract: Syslog ingest handler

**Feature**: `009-admin-ui-ingest` | Handler name: `syslog` (reserved in `001`) | `crates/ingest/src/syslog.rs`

## Entrypoint

Exactly one handler per listen address:port. Child `ingest { namespace; container; type?; }` is mandatory. That port writes **only** to that target (clarify Q3). HOSTNAME, APP-NAME, and facility are stored on the record; they MUST NOT select another container (FR-008).

UDP and TCP on the same plaintext port. `tls { … }` → TCP only (no silent plaintext).

## Framing

| Transport | Preferred | Fallback |
|-----------|-----------|----------|
| TCP | RFC 5424 octet-counted | RFC 5424 non-transparent (newline); then RFC 3164 line |
| UDP | RFC 5424 datagram | RFC 3164 datagram |

BOM/UTF-8 as in RFC 5424. Max datagram/line: `ingest.syslog.recv` capacity; larger → drop + metric, connection close on TCP.

## Burst / overflow

Policy **reject** (R7): drop, increment `spacestorage_ingest_dropped_total{source="syslog",reason="buffer_full"}`, do not block Tokio workers. TCP: close after failed enqueue. UDP: drop datagram.

## Drain

Node `draining`: stop accepting new syslog connections/datagrams; finish in-flight appends up to drain timeout (`001`).

## Distinct from export

Outbound syslog (`008`) uses APP-NAME `spacestorage`. Ingest does not emit on that path. A loop (export → this port) is allowed and becomes stored data; it MUST NOT deadlock workers.
