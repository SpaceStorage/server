# Implementation Plan: Wire Protocols and Datatype-Aware Drivers

**Branch**: `002-protocol-drivers` | **Date**: 2026-09-13 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/002-protocol-drivers/spec.md` (with Clarifications, Session 2026-09-13)

## Summary

Add eight client-facing protocol handlers to the `001` node — `postgresql`, `cassandra`, `redis`, `elasticsearch`, `clickhouse` (native), `clickhouse-http`, `s3`, `webdav` — each registered in the existing `HandlerRegistry` and declared on its own entrypoint. Every handler is a thin transport adapter over a **driver** that (a) authenticates through one `Authenticator` seam and binds the session to exactly one namespace (client-selected for PostgreSQL/ClickHouse/Cassandra, credential-bound for Redis/S3/WebDAV/Elasticsearch), (b) lowers the protocol's own language or verbs into a protocol-neutral `LogicalRequest`, (c) attaches `QueryOptions` (Cassandra-vocabulary quorum + timeout, resolved query → session → global with default-clamping / explicit-rejection), and (d) submits to the single `QueryEngine`, rendering results and errors in the protocol's native shapes. Drivers see data only through the **abstract datatype interface** (`TypeRegistry`/`Datatype`/`Container`); every type is listable, creatable (type option on the native create verb), readable, writable, alterable, droppable and operable from every protocol, natively where a faithful equivalent exists and otherwise via one **canonical JSON representation** carried unaltered. Because `03` (types), `05` (execution) and `07` (roles) are not yet specified, this feature ships those three surfaces as stable traits with interim implementations (six in-memory types, a single-node `LocalEngine`, a `users_file` authenticator) that the sibling features replace behind the same traits. All code is pure Rust on the `001` Tokio runtime.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same workspace as `001`.

**Primary Dependencies** (all pure Rust; `*-sys` crates prohibited): `pgwire` (+`server-api-scram`) and `sqlparser` (PostgreSQL + ClickHouse dialects); `cassandra-protocol` (CQL v4/v5 frame codecs) with a hand-written CQL parser; `redis-protocol` (RESP2/3); `axum` + `hyper` 1.x (Elasticsearch, ClickHouse-HTTP, S3, WebDAV, shared with `admin-http`); `quick-xml` (S3/WebDAV XML); `hmac`, `sha2`, `md-5`, `hex`, `base64` (SigV4, Digest, ETags, canonical bytes); `lz4_flex` (CQL and ClickHouse compression), in-house CityHash 1.0.2; `flate2` (`miniz_oxide` backend) for HTTP gzip; `serde`/`serde_json`, `ryu` (canonical JSON); `tokio`, `tokio-util`, `futures`, `bytes`, `async-trait`, `tracing`, `uuid`, `chrono`. Dev-dependencies (conformance "stock clients"): `tokio-postgres`, `scylla`, `redis`, `elasticsearch`, `clickhouse`, `reqwest`, `aws-sdk-s3` (+`aws-config`), `reqwest_dav`.

**Storage**: In-memory only in this feature (interim type inventory accounted in buffer `types.memory`); durable L0 storage and persistence belong to `03`. Nothing survives restart until `03` lands — documented limitation.

**Testing**: `cargo test` — unit tests per crate (parsers, codecs, canonical vectors, SigV4/CityHash test vectors); `crates/conformance/` integration suite boots one node in-process with all eight handlers on ephemeral ports and drives it with the client crates above: per-handler smoke (SC-001), type × protocol matrix (SC-002/003), option recording via admin `executions` (SC-004), timeout deadline (SC-005), cross-protocol equivalence (SC-006), mismatch matrix (SC-008), namespace isolation, drain; contract tests validate every fixture under `contracts/fixtures/`; `quickstart.md` gives the CLI-tool walkthrough.

**Target Platform**: Linux x86_64/aarch64 servers; macOS for development.

**Project Type**: Cargo workspace extension — library crates for the three seams (`types`, `exec`, `auth`), one shared driver toolkit crate (`protocol-core`), seven handler crates (ClickHouse crate hosts two handlers), one conformance crate; no new binaries.

**Performance Goals**: SC-001 smoke workflows pass with stock clients; SC-005 timeout error within deadline + 1 s (target + 50 ms); SC-008 mismatch refusal < 1 s (target < 10 ms); handshake + auth < 50 ms p95 on loopback; per-connection memory bounded by `protocol.<h>.recv/send` buffers; 10 000 concurrent idle sessions per node without runtime degradation (Principle II/III).

**Constraints**: One handler per entrypoint (no transport multiplexing); every listener declared; first-bytes signature check with 1 s deadline before any session allocation; drivers touch data only through `spacestorage-types` and `spacestorage-exec` traits (compile-time enforced by crate dependency graph: handler crates do not depend on any storage or engine internals); canonical bytes never altered by a driver; credentials referenced from a `0600` file, never inlined; no plaintext fallback on TLS entrypoints (inherited); every executed request produces an `ExecutionRecord`.

**Scale/Scope**: 8 handlers / 7 protocols; 6 interim types; `LogicalRequest` with 9 variants; ≈ 11 new crates; largest items are the ClickHouse native protocol (~3.5k lines), CQL parser + server (~3k), S3 (~2.5k), WebDAV (~2k), Elasticsearch (~2.5k), PostgreSQL lowering (~1.5k on top of `pgwire`), Redis (~1.5k); ≈ 25–30k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | Every new dependency is pure Rust; `clickhouse-rs-cityhash-sys` (C++) rejected in favour of in-house CityHash 1.0.2; `zstd`/`brotli` HTTP encodings omitted (C-backed); dev-only client crates may pull `ring`/`aws-lc-rs` but never ship | PASS |
| II | Fully Asynchronous Tokio Runtime | All handlers run inside `Handler::serve` on the `001` runtime; parsers are CPU-bound and short; SCRAM/SigV4 HMACs are microsecond-scale; no blocking I/O (users file via `tokio::fs`) | PASS |
| III | Single-Process Multithreaded Monolith | Handlers are crates linked into `spacestoraged`; no sidecar processes; no new binaries | PASS |
| IV | Type-Driven Multiparadigm | Drivers consume `TypeRegistry`; listing, create-option validation, type-op dispatch and canonical schemas are registry-driven, so a new type needs no driver change. Interim six-type inventory lives in `spacestorage-types` (the crate `03` will own), not in any driver | PASS (interim inventory tracked below) |
| V | Protocol Compatibility on Distinct Ports | Eight handlers registered; `entrypoint_duplicate_address` already forbids two handlers on one `address:port`; ClickHouse split into two handlers per Q5; no transport auto-detection | PASS |
| VI | Every Node Is a Request Coordinator | All handlers are available on every node; `LocalEngine` answers every request locally; `PlacementInfo` seam lets `04`/`06` route remote data without driver changes | PASS |
| VII | Label-Based Planetary Placement | Not constrained here; `PlacementInfo::replicas/satisfiable/clamp` is the only placement-facing surface and is owned by `04` | PASS (N/A) |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Shared vocabulary; `query_defaults { write_quorum TWO; read_quorum ONE; }`; every `QueryOptions` carries `Sourced<QuorumLevel>` + `Sourced<Duration>`; native fields honoured where they exist; defaults clamp / explicit reject per Q1 | PASS |
| IX | Multi-Tenant Namespaces | Every session bound to exactly one namespace; credential-bound on protocols without selection (Q2); `ContainerRef{namespace, schema, name}` everywhere; per-namespace metric labels reserved | PASS |
| X | Observability as a Product Surface | Per-protocol counters/histograms with `08` label names (`protocol`, `namespace`, `user`, `app`, `kind`, `error_type`) tracked in `node::stats`; `ExecutionRecord` ring; exposition still `08`'s | PASS |
| XI | Documented, Expandable Configuration | `contracts/config-directives.md`, `fixtures/node-all-protocols.conf` (SC-010), `users.example`, per-protocol contracts with type-mapping and option tables; handler inventory, type registry, and `protocols {}` knobs are open-ended | PASS |
| XII | Raft Controller Elections and Local Restore | Not applicable; WebDAV locks are stored as a normal container so restore semantics come from `03`/`06` | PASS (N/A) |
| XIII | Security Defaults for Data and Roles | Role system absent (`07`). Interim `auth { users_file }` (0600, plaintext secrets required by SCRAM/SigV4/Digest) behind the `Authenticator` trait; `admin` role allows any-namespace selection on SQL/CQL protocols only; all protocol entrypoints may declare TLS. Tracked in Complexity Tracking | PASS with justified deviation |
| Arch. Contracts | Protocol adapter ≠ type system; quorum/timeout/namespace expressible everywhere | Handler crates depend only on `protocol-core`, `types`, `exec`, `auth` traits; canonical fallback and `SS.*`/`_spacestorage`/`spacestorage.*` surfaces give every protocol every type, option and namespace | PASS |
| Observability Contract | No `08` label renamed or dropped | Only additions (`protocol`, `user`, `app`, `kind`, `error_type` per `08`'s own lists) | PASS |

**Gate result (pre-research)**: PASS. Deviations justified below: interim authenticator (XIII), interim type inventory and engine (sequencing against `03`/`05`).

## Project Structure

### Documentation (this feature)

```text
specs/002-protocol-drivers/
├── plan.md                                  # This file
├── research.md                              # Phase 0: R1–R18
├── data-model.md                            # Phase 1: config additions, identity, type seam, options, sessions, IR, mappings, error forms
├── quickstart.md                            # Phase 1: stock-client walkthrough per SC
├── contracts/
│   ├── abstract-datatype-interface.md       # TypeRegistry / Datatype / Container / ObjectOps (03 seam)
│   ├── execution-boundary.md                # LogicalRequest / QueryOptions / QueryEngine / PlacementInfo (05 seam)
│   ├── canonical-representation.md          # canonical JSON v1, carriers, payload schemas
│   ├── query-options.md                     # vocabulary, precedence, clamping, per-protocol syntax + inspection
│   ├── config-directives.md                 # query_defaults, auth, protocols{}, buffers, effective config, CLI additions
│   ├── protocols/{postgresql,cassandra,redis,elasticsearch,clickhouse,s3,webdav}.md
│   └── fixtures/
│       ├── node-all-protocols.conf          # SC-010 starter
│       ├── users.example
│       ├── README.md
│       └── invalid/*                        # one file per new validation code
├── checklists/requirements.md
└── tasks.md                                 # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
Cargo.toml                                   # workspace: adds the crates below
crates/
├── config/                                  # (001) + feature-registered blocks: query_defaults, auth, protocols{}; new validation codes
├── node/                                    # (001) + handler registration for 02; stats: protocol counters; admin op `executions`
├── admin-proto/                             # (001) + ExecutionRecord, ProtocolMapEntry types; op `executions`, `protocols`
├── spacestorage/                            # (001 CLI) + `protocols`, `executions` commands
│
├── types/                                   # spacestorage-types — 03 SEAM (interim provider)
│   └── src/
│       ├── lib.rs                           # TypeRegistry, TypeName, Level, TypeError
│       ├── datatype.rs                      # Datatype, Container, ObjectOps traits; ScanQuery/AggQuery/Expr eval helpers
│       ├── canonical.rs                     # CanonicalValue, Envelope, to/from_canonical_bytes (RFC 8785-style)
│       ├── ident.rs                         # NamespaceName, SchemaName, ContainerName, ContainerRef
│       ├── catalog.rs                       # in-memory namespace → schema → container metadata; implicit namespace creation
│       └── interim/                         # six in-memory types (removed/replaced by 03)
│           ├── kv_collection.rs  relational_table.rs  document_store.rs
│           ├── object_collection.rs  vector_collection.rs  ordered_map.rs
│           └── memory.rs                    # accounting against buffer `types.memory`
│   └── tests/canonical_vectors/*.json, canonical.rs, interim_types.rs
│
├── exec/                                    # spacestorage-exec — 05 SEAM (interim provider)
│   └── src/
│       ├── lib.rs                           # QueryEngine, ExecutionEvent, ExecError, PlacementInfo
│       ├── request.rs                       # LogicalRequest, Ddl, Point, Scan, Aggregate, Join, Mutate, TypeOp, ObjectReq, Expr
│       ├── options.rs                       # QuorumLevel, Sourced<T>, QueryOptions, resolve(), clamp()
│       ├── record.rs                        # ExecutionRecord ring (buffer `exec.records`)
│       └── local/                           # LocalEngine: single node, direct TypeRegistry execution
│           ├── mod.rs  ddl.rs  point.rs  scan.rs  aggregate.rs  join.rs  mutate.rs  typeop.rs  object.rs  timeout.rs
│   └── tests/{options.rs, local_engine.rs, records.rs}
│
├── auth/                                    # spacestorage-auth — 07 SEAM (interim provider)
│   └── src/
│       ├── lib.rs                           # Authenticator trait, Credential, Principal, AuthError
│       ├── users_file.rs                    # parse/validate/reload `auth { users_file }`; 0600 check
│       ├── scram.rs                         # SCRAM-SHA-256 verifier derivation for pgwire
│       ├── sigv4.rs                         # canonical request, string-to-sign, signing key, chunked payload
│       └── digest.rs                        # RFC 2617/7616 MD5 digest
│   └── tests/{users_file.rs, sigv4_vectors.rs, digest.rs}
│
├── protocol-core/                           # shared driver toolkit
│   └── src/
│       ├── lib.rs                           # ProtocolDriver trait, ProtocolName, Mapping/Carrier
│       ├── session.rs                       # ClientSession, SessionOptions, state machine, drain hooks
│       ├── options.rs                       # option literal parsing, precedence application, inspection rows
│       ├── signature.rs                     # first-bytes protocol detection with deadline (FR-004)
│       ├── namespace.rs                     # binding rules (client-selected vs credential-bound, admin override)
│       ├── errors.rs                        # ErrorRenderer trait + per-class mapping helpers
│       ├── http.rs                          # axum helpers: Basic/Digest extractors, X-SpaceStorage-* headers, per-connection session state, hyper serve_connection glue
│       ├── stats.rs                         # protocol metric names/labels (reserved for 08)
│       └── escape.rs                        # reversible name escaping helpers
│
├── handler-postgresql/                      # `postgresql`
│   └── src/{lib.rs, handler.rs (pgwire glue, SSLRequest/TLS), auth.rs, lower/{mod.rs, ddl.rs, dml.rs, select.rs, options.rs, catalog.rs}, render.rs (rows/types/errors), catalog_views.rs (spacestorage.containers, pg_catalog min)}
├── handler-cassandra/                       # `cassandra`
│   └── src/{lib.rs, handler.rs (frames v4/v5, compression), auth.rs, cql/{lexer.rs, parser.rs, ast.rs}, lower.rs, render.rs, system_tables.rs, prepared.rs, events.rs}
├── handler-redis/                           # `redis`
│   └── src/{lib.rs, handler.rs, auth.rs, commands/{mod.rs, server.rs, string.rs, hash.rs, keys.rs, ss.rs}, lower.rs, render.rs}
├── handler-elasticsearch/                   # `elasticsearch`
│   └── src/{lib.rs, router.rs, auth.rs, api/{root.rs, cluster.rs, cat.rs, index.rs, doc.rs, bulk.rs, search.rs, aggs.rs, spacestorage.rs}, dsl/{query.rs, lower.rs}, render.rs}
├── handler-clickhouse/                      # `clickhouse` + `clickhouse-http`
│   └── src/{lib.rs, native/{handler.rs, packets.rs, block.rs, column_types.rs, compression.rs, cityhash102.rs, hello.rs, query.rs}, http/{router.rs, formats/{tsv.rs, csv.rs, json.rs, row_binary.rs, native.rs, pretty.rs}, session.rs}, sql/{lower.rs, functions.rs}, system_tables.rs, render.rs}
├── handler-s3/                              # `s3`
│   └── src/{lib.rs, router.rs, sigv4_extract.rs, xml.rs, ops/{bucket.rs, object.rs, list.rs, multipart.rs, spacestorage.rs}, render.rs}
├── handler-webdav/                          # `webdav`
│   └── src/{lib.rs, router.rs, auth.rs, methods/{propfind.rs, proppatch.rs, mkcol.rs, copy_move.rs, lock.rs, report.rs, get_put_delete.rs}, props.rs, locks.rs, xml.rs, render.rs}
│
└── conformance/                             # spacestorage-conformance (tests only)
    └── tests/
        ├── harness/mod.rs                   # boot node with 8 handlers on port 0; users file; client factories
        ├── smoke_{postgresql,cassandra,redis,elasticsearch,clickhouse,clickhouse_http,s3,webdav}.rs   # SC-001
        ├── type_matrix.rs                   # SC-002, SC-003 (every type × every protocol; canonical write-back)
        ├── options.rs                       # SC-004 (defaults, clamping, explicit rejection, precedence, inspection)
        ├── timeout_deadline.rs              # SC-005
        ├── equivalence.rs                   # SC-006
        ├── mismatch_matrix.rs               # SC-008
        ├── namespace_isolation.rs           # Story 1 scenarios 12–13
        ├── drain.rs                         # FR-040
        ├── cross_node_read_after_write.rs   # SC-007 — #[ignore] until 04/06
        └── fixtures.rs                      # contracts/fixtures validation (SC-010)

docs/
├── protocols/{postgresql,cassandra,redis,elasticsearch,clickhouse,s3,webdav}.md   # generated from contracts at release (type-mapping tables, option syntax)
└── examples/node-all-protocols.conf
```

**Structure Decision**: Three seam crates (`types`, `exec`, `auth`) hold the traits that features `03`, `05`, `07` will implement, plus interim implementations gated behind the same traits. `protocol-core` collects everything shared by drivers (sessions, option resolution, signature detection, error rendering, HTTP glue, metrics names) so handler crates contain only protocol-specific code. Handler crates depend on `protocol-core`, `types`, `exec`, `auth` **only** — the dependency graph is the enforcement of FR-012/FR-035. ClickHouse is one crate because both handlers share the SQL lowering, block encoder and type mappings; it registers two `Handler` implementations. `conformance` is a test-only crate so client-crate dev-dependencies never enter the server build.

## Complexity Tracking

| Violation / deviation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Interim users-file authenticator with plaintext secrets (XIII) | FR-010 requires every protocol's native auth exchange to resolve against the role system now; `07` does not exist. SCRAM-SHA-256, SigV4 and Digest need the secret (or a scheme-specific derivative) server-side | Hashed-only secrets make SigV4/Digest impossible; anonymous loopback access violates FR-010. The file is `0600`, referenced by path, live-reloadable, and `Authenticator` is the single seam `07` replaces. |
| Interim six-type in-memory inventory inside `spacestorage-types` (IV, sequencing vs `03`) | FR-011–FR-018 and Story 2 cannot be built or tested without concrete types; `03` is unspecified | Waiting blocks the intent's numbered order; placing types in drivers would create the forbidden per-protocol type systems. The inventory lives in the crate `03` owns and is registry-driven so drivers are unaffected when `03` replaces it. Data is not durable until `03` (documented). |
| Interim `LocalEngine` (sequencing vs `05`) | FR-035 requires every driver to submit to one engine; `05` is unspecified | Per-driver execution is exactly the protocol-private engine FR-035 forbids. `LocalEngine` implements the `QueryEngine` trait `05` will implement, is single-node, and records every request. |
| Hand-written ClickHouse native protocol incl. CityHash 1.0.2 (size) | Q5 requires the native transport; no pure-Rust server implementation exists and the only checksum crate is C++ | Using `clickhouse-rs-cityhash-sys` violates Principle I; HTTP-only violates Q5. |
| Hand-written CQL parser instead of `sqlparser` | CQL semantics (partition/clustering keys, `USING`, `IF`, `ALLOW FILTERING`, custom payload options) are not modelled by SQL parsers | Bending `sqlparser` would misparse valid CQL and produce SQL-shaped errors; CQL's grammar is small (~1.2k lines). |
| Eight handler surfaces with SpaceStorage extensions (`SS.*`, `_spacestorage`, `spacestorage.*`, `?spacestorage-op`, `REPORT`) | Q3/Q4 require full type lifecycle and type-specific operations from every protocol; stock protocols have no verbs for foreign types | Restricting foreign types to their "home" protocol was rejected in Q3; the extensions follow each protocol's own extension convention (module commands, `_`-prefixed endpoints, schema-qualified functions, query params, DAV reports) so stock clients pass them through. |

## Phase 0 — Research

See [research.md](research.md), R1–R18. All Technical Context items are resolved; no `NEEDS CLARIFICATION` remains. Key outcomes: three seams with interim implementations (R1, R11, R15, R16); protocol stacks (R2–R8); canonical JSON v1 (R9); option syntax and inspection per protocol (R10); addressing and type-option syntax (R12, R13); signature detection (R14); reserved metrics (R17); stock-client conformance strategy (R18).

## Phase 1 — Design

- [data-model.md](data-model.md): configuration additions, identity/addressing, type seam, query options with `Sourced<T>`, session state machine, execution IR, driver mapping model, per-protocol error form table, relationships.
- [contracts/](contracts/): abstract datatype interface, execution boundary (with protocol-construct → `LogicalRequest` table), canonical representation, query options, config directives, seven per-protocol contracts, fixtures.
- [quickstart.md](quickstart.md): stock-client walkthrough covering every success criterion except SC-007 (deferred to `04`/`06`).

## Constitution Check (post-design)

Re-evaluated after Phase 1: no new violations. Design keeps every dependency pure Rust, all work on the `001` runtime, one handler per port (ClickHouse as two handlers), registry-driven type exposure with a single canonical fallback, quorum/timeout/namespace on every request with `08`-compatible labels, documented starter configuration, and the three interim seams tracked above. **Gate result: PASS.**

## Risks and sequencing notes

- **Durability gap**: until `03`, all containers are in-memory. The quickstart and docs state this; conformance tests do not assume persistence across restarts.
- **Version drift of emulated protocols**: contracts pin the announced versions (PG `16.4`, ES `8.15.0`, CH revision `54460`, CQL v4/v5, Redis `7.2.0`); the conformance suite pins client crate versions; bumping either is an explicit task.
- **SC-007** cannot be verified until cluster membership exists; the test is written and `#[ignore]`d with the reason.
- **Users file secrets**: interim only; `07` must define migration (the `Authenticator` seam accepts scheme-specific verifiers so hashed storage becomes possible once SigV4/Digest provisioning is designed).
- **Order of implementation** (for `/speckit-tasks`): seams (`types`, `exec`, `auth`) and `protocol-core` first; then `redis` (smallest, validates the whole path); `postgresql` (pgwire); `elasticsearch`, `s3`, `webdav` (shared HTTP glue); `cassandra`; `clickhouse` last (largest); conformance suite grows with each handler.

## Next Step

Run `/speckit-tasks` to generate `tasks.md` from this plan and the Phase 1 artifacts.
