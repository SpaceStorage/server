# Milestone: complete-product (slices 1–11)

**Profile**: `complete-product`  
**Tag**: `slices-1-11`

## Implemented

Slices `1..=11` shipped. Ledger: [007-complete-product.yaml](007-complete-product.yaml).

Slice **11** DoD (`009-admin-ui-ingest`):

- New crates `spacestorage-admin-ui` (Cerebro-like map + Kibana-like console) and `spacestorage-ingest` (Kafka consumer + syslog)
- Admin HTTP: `/ui/*`, `/v1/cluster/map`, `/v1/console/*`, `/v1/ingest/kafka`, `/v1/auth/login`
- CLI: `spacestorage ui`, `spacestorage ingest kafka|syslog`
- Config: `ingest kafka`, `entrypoint.ingest`, buffers `ingest.syslog.recv` / `ingest.kafka.decode`
- First-binary refuses UI/ingest with `UiIngestSlice11Required`
- Kafka at-least-once (OffsetCommit after durable ack); syslog RFC 5424 preferred / 3164 accepted; 1:1 port→container
- Release profile / Cargo feature `complete-product` is the merge gate for the full matrix (not a synonym for handlers-only)
- Conformance behind `--features complete-product`

## Deferred

None for slices 6–11 (`still_owed` cleared on the tip ledger). Honest residuals that do **not** keep slice 11 `still_owed`:

- ~~Live Kafka broker I/O beyond `FakeKafkaBroker`~~ — **done (2026-09-27)**: `LiveKafkaClient` (`crates/ingest/src/kafka_wire.rs`) does Metadata/Fetch/OffsetFetch/OffsetCommit over TCP via `kafka-protocol`; `FakeKafkaBroker` remains for ALO unit/conformance tests; `KafkaConsumer::poll_one_async` covers live path.
- ~~Kafka JoinGroup/SyncGroup + range assignor~~ — **done (2026-09-27)**: live bootstrap runs FindCoordinator → JoinGroup (MEMBER_ID_REQUIRED retry) → SyncGroup with classic **range** assignor (`ConsumerProtocolSubscription` / `ConsumerProtocolAssignment`); partition set for Fetch comes from the SyncGroup assignment.
- ~~Real OTLP protobuf~~ — **done**: hand-rolled `ExportMetricsServiceRequest` encoder + HTTP/1.1 POST (`application/x-protobuf`) in `crates/observability` (`otlp_encode` / `otel`); capture mode for unit tests.
- ~~OTLP https TLS client~~ — **done (2026-09-27)**: `rustls` + `webpki-roots` + `tokio-rustls` client in `otel::http_post_protobuf` for `https://` collectors (http path unchanged).
- ~~Outbound Kafka Produce~~ — **done**: real ProduceRequest framing + TCP Produce in `KafkaProducer`; `KafkaProducer::memory()` keeps conformance offline while still encoding wire frames.
- ~~openraft `Raft::new` / 0.10~~ — **done (2026-09-27)**: bumped to `openraft = 0.10.0-alpha.35` (+ transitive `validit` / `quorum-set`); workspace MSRV **1.88**; `RaftGroup` owns a real `Raft::new` runtime (`openraft_adapter::start_raft` + mem log/SM); single-voter bootstrap uses `initialize` + wait-for-leader; `client_write` on the solo path; SpaceStorage on-disk `RaftStore` format retained for restore/read-index. Multi-node openraft peer RPCs still stubbed as unreachable (in-process harness RPCs remain on `RaftGroup`).
- Post-ladder hardening (2026-09-26): WAL kill→reboot→re-read, PG Document Store blob, `/ui/*` `014` SessionToken via `POST /v1/auth/login` — see changelog

## Changelog

- Added admin-ui and ingest library crates; mounted on admin-http
- Wired slice-11 feature through release-profile, node, spacestorage CLI, and conformance
- Closed the full-product ladder (intent `16` slices 1–11)
- **2026-09-26 post-ladder**: handlers Cassandra/CH/ES/S3/WebDAV (+ Redis data verbs) lower to shared `PlannerEngine` IR (005 T084–T086); WAL durable hooks + definitions persist for kill→reboot client re-read; PG Document Store blob gated like Redis G11; UI auth uses `014` `SessionTokenStore` / `AuthLogin` (static admin token alone no longer grants `/ui/*`)
- **2026-09-27 production I/O residuals**: live Kafka ingest client; OTLP protobuf push; outbound Kafka Produce; openraft 0.9.25 pin + adapters (0.10 then blocked on rustc ≥ 1.88)
- **2026-09-27 deferred residuals landed**: openraft 0.10.0-alpha.35 + `Raft::new` (MSRV 1.88); Kafka JoinGroup/SyncGroup range assignor; OTLP HTTPS rustls client
