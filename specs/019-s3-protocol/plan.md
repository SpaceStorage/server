# Implementation Plan: S3 Protocol (Complete Product)

**Branch**: `019-s3-protocol` | **Date**: 2026-11-23 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/019-s3-protocol/spec.md` (APPROVED; clarify returned 0 questions after expansion). Clarifications table and FR-001–FR-061 are authoritative for the complete-product S3 dialect. Wire shapes in `specs/002-protocol-drivers/contracts/protocols/s3.md` inform implementation; **FR-011** makes this feature own the S3 capability table and matrix for merge/conformance.

## Summary

Promote the existing HandlersComplete smoke in `crates/handler-s3` into a **production-quality S3 REST dialect** for profiles that already include the `s3` handler (`HandlersComplete` and later; **not** FirstBinary). The handler listens on its own entrypoint port, authenticates with full **AWS SigV4** (header, presigned query, `UNSIGNED-PAYLOAD` and streaming payload modes), scopes every session to one **014** namespace-bound principal, and maps buckets/objects onto the shared datatype model (default `object_collection`).

Protocol semantics (ETag rules, metadata, ranges, conditionals, XML errors), path-style and optional virtual-hosted addressing, and **streaming I/O without full-object RAM** are first-class. Coverage is proven by two independent live-server suites: **Suite A** (protocol conformance in `crates/conformance`) and **Suite B** (Docker compose of eight stock client images with SHA256 verify). A three-status **compatibility matrix** is generated from the capability table plus suite results.

First-binary builds stay unchanged: `s3` remains in the FirstBinary forbidden set (`crates/release-profile`). Docker Suite B is **test tooling** (client images targeting an external `spacestoraged`), not a second server implementation.

## Technical Context

**Language/Version**: Rust edition 2024, `rust-version` 1.88 (workspace `Cargo.toml`). Same Cargo workspace as `001`–`016`.

**Primary Dependencies** (server, all Rust): existing `tokio`, `bytes`, `hmac`/`sha2`/`subtle`, `hex`; add `quick-xml` (XML list/error/complete-multipart bodies), `md-5` (ETag / Content-MD5), CRC crates already acceptable as pure Rust (`crc32fast`, `crc32c` or equivalent) for checksum headers; `spacestorage-{compat,protocol-core,query,types,tenancy,authz,storage,placement}`. No new C/`*-sys` deps in the server graph. Suite B / boto3 live outside Cargo (Docker + Python images). Optional Suite A sibling may use `aws-sdk-s3` as a **dev-dependency** of `spacestorage-conformance` only (same pattern as `002` research).

**Storage**: Buckets = namespace-scoped containers (default type `object_collection` per FR-010 / `002` s3 contract). Object bodies and multipart parts go through the shared datatype / storage path (`003`/`013`); durable write quorum waits for the durability contract, not an in-memory buffer alone. Multipart upload state is a **namespace-scoped durable registry** (not a per-TCP-connection `HashMap`).

**Testing**: Suite A — `cargo test -p spacestorage-conformance` behind `handlers-complete` / `complete-product` features as appropriate, live in-process or live-TCP node. Suite B — Docker Compose under `tests/s3-interop/` against an already-running server. Every successful write records fixture SHA256 and verifies on read. Matrix generator consumes Suite A/B JUnit/JSON + [contracts/capability-matrix.md](contracts/capability-matrix.md).

**Target Platform**: Linux x86_64/aarch64 servers (primary); macOS for development. Suite B requires Docker locally/CI.

**Project Type**: Extension of existing crates (`handler-s3`, `compat`, `node`, `authz`, `conformance`, `config`) plus a new test harness tree `tests/s3-interop/`. No new daemon binary and no new listen-port family beyond the existing `s3` entrypoint.

**Performance Goals**: SC-007 conformance ceiling (research R6): peak RSS during large-object streaming MUST NOT grow as a linear function of object size — object size MUST NOT appear as a linear term in measured peak RSS for the documented large-object fixture (FR-032). FR-054 / US9 runs **N = 8** concurrent upload/download workers, and all eight MUST complete with correct SHA256; eight is the CI count because a typical shared runner has few cores and a small FD budget, and eight streams still prove concurrent transfer. SigV4 verify stays on the order of microseconds for typical header sets; clock skew window 15 minutes.

**Constraints**: Protocol-per-port; unbound credentials refused (014); FirstBinary must not link or listen S3; `classify` before IR; MustNot / Unsupported never silent success; `max_key` / `max_value` / connection caps from `015` enforced with named S3 errors; multipart min part 5 MiB (except last), max 10 000 parts (`002`/`015`); operator MAY raise `limits.max_value` for multi-GB backups (Suite B fixtures do). No full-object `Vec<u8>` on the streaming path. Implement MUST map `X-SpaceStorage-Quorum` and `X-SpaceStorage-Timeout` (session header when set; administrator / `query_defaults` when unset) into `QueryOptions` on every S3 request (FR-009, constitution VIII). Catalog and durable `execute_blocking` in `handler-s3` MUST move to `spawn_blocking` / the storage durability pool, including `dispatch.rs`, not only `stream.rs` (research R8, constitution II).

**Scale/Scope**: ~20 Supported-and-tested operations; SigV4 surface (3 payload modes + presign); 8 Suite B tools; one second AWS SDK smoke; matrix with three statuses. Roughly 4–8k net new lines in `handler-s3` + authz wiring + conformance modules; Suite B scripts/compose separate.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate for this feature | Status |
|---|-----------|------------------------|--------|
| I | Rust-Only Stack | Server and CLI remain Rust-only. Suite B Docker images and boto3/Python are **client test tooling**, not server dependencies (parallel to `002` stock-client crates as test-only). No second S3 server in another language. | PASS |
| II | Fully Asynchronous Tokio Runtime | Handler `serve` is async. Body upload/download streams asynchronously. Fsync / durable quorum waits use `spawn_blocking` (or the existing storage durability pool). Plan forbids buffering entire large objects on the Tokio worker before writing. Catalog and durable `execute_blocking` in `handler-s3` MUST move onto `spawn_blocking` / the storage durability pool, including `dispatch.rs`, not only `stream.rs`, so a durable wait does not occupy a Tokio worker (research R8). | PASS |
| III | Single-Process Multithreaded Monolith | S3 is one handler crate linked into `spacestoraged`. Suite B does not spawn a second SpaceStorage process inside client images. | PASS |
| IV | Type-Driven Multiparadigm | Buckets/objects use `object_collection` (and documented type options); handler is a protocol adapter over `LogicalRequest` / datatype traits — not a parallel object engine. | PASS |
| V | Protocol Compatibility on Distinct Ports | S3 keeps its own entrypoint; `entrypoint_duplicate_address` already forbids sharing. Path-style always; vhost when `virtual_host_suffix` is set. | PASS |
| VI | Every Node Is a Request Coordinator | Any node with an `s3` entrypoint serves the dialect; placement/quorum follow `004`/`012`/`013`. | PASS |
| VII | Label-Based Planetary Placement | No new farness keys. Object placement uses existing container placement; `planet` stays a label. | PASS (N/A) |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Implement MUST map `X-SpaceStorage-Quorum` and `X-SpaceStorage-Timeout` into `QueryOptions` on every S3 request (session header when set; administrator / `query_defaults` when unset) (FR-009). Write TWO counts only durable replicas in the source `quorum_domain`. | PASS |
| IX | Multi-Tenant Namespaces | Credential-bound namespace only; unbound (incl. unbound `CLUSTER_ADMIN`) refused at auth; bucket names unique per namespace. | PASS |
| X | Observability as a Product Surface | `protocol="s3"`; `kind` = operation name; existing `008` label set; no renames. | PASS |
| XI | Documented, Expandable Configuration | `protocols.s3.*` knobs (`virtual_host_suffix`, region constant, multipart idle) documented in contracts; matrix is expandable. | PASS |
| XII | Raft Controller Elections and Local Restore | Multipart registry and object data restore with datatype durability (`013`); no S3-specific Raft. | PASS (N/A) |
| XIII | Security Defaults for Data and Roles | SigV4 → `014` principal store / access keys; session tokens when issued; SSE-as-AWS-KMS stays Unsupported by design until product encryption mapping says otherwise. | PASS |
| Arch. Contracts | Adapter ≠ type system | `handler-s3` depends on protocol-core / types / query / authz / compat — not a private storage fork. | PASS |
| Observability Contract | Intent `08` labels | Additive series only if needed; required labels unchanged. | PASS |

**Gate result (pre-research)**: PASS. Docker Suite B and Python boto3 are recorded in Complexity Tracking as **test tooling**, not principle waivers. Streaming + `spawn_blocking` for durability is the constitution-aligned design for FR-032.

**Gate result (post-Phase 1 design)**: PASS. Contracts and data model keep the adapter boundary, namespace binding, protocol-per-port, and durable-write path. Clarifications recorded here, still PASS: SC-007 peak-RSS rule and **N = 8** (Performance Goals); every S3 request maps quorum and timeout headers into `QueryOptions` (VIII / FR-009); `handler-s3` catalog/durable `execute_blocking`, including `dispatch.rs`, moves to `spawn_blocking` / the storage durability pool (II / R8). No new Complexity Tracking rows beyond the test-tooling note.

## Project Structure

### Documentation (this feature)

```text
specs/019-s3-protocol/
├── plan.md                 # This file
├── research.md             # Phase 0 (R1–R16)
├── data-model.md           # Phase 1 entities
├── quickstart.md           # Phase 1 validation guide
├── contracts/
│   ├── capability-matrix.md    # Three-status table (FR-011)
│   ├── operations.md           # Supported HTTP/API shapes
│   ├── sigv4.md                # Auth, payload modes, skew, presign
│   ├── errors.md               # XML Error codes + HTTP mapping
│   ├── config-directives.md    # protocols.s3.* + limits interaction
│   ├── suite-a.md              # Protocol conformance contract
│   ├── suite-b.md              # Docker interop harness contract
│   └── fixtures/
│       └── README.md
├── checklists/requirements.md
└── tasks.md                # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── handler-s3/                         # Expand from HC smoke → CP dialect
│   └── src/
│       ├── lib.rs
│       ├── handler.rs                  # HTTP/1.1 serve, keep-alive, 100-continue, chunked, vhost
│       ├── router.rs                   # Path + query → verb (Supported / Unsupported / NYI)
│       ├── dispatch.rs                 # classify → LogicalRequest / ObjectOps; QueryOptions from quorum/timeout headers; catalog execute_blocking on the durability pool
│       ├── sigv4.rs                    # Header + query + streaming payload verify
│       ├── etag.rs                     # Single-part + composite multipart ETag
│       ├── checksum.rs                 # Content-MD5 + x-amz-checksum-*
│       ├── xml.rs                      # List*/Error/CompleteMultipart bodies
│       ├── multipart.rs                # Upload registry + part lifecycle
│       └── stream.rs                   # Async body pump ↔ storage (no full-object RAM); durable waits share the same blocking pool as dispatch.rs
├── compat/                             # classify_s3: full Supported set; MustNot for Unsupported/NYI
├── authz/                              # SigV4 access-key → principal; session token; namespace bind
├── config/                             # protocols.s3.virtual_host_suffix, region, multipart_idle
├── node/                               # S3PortHandler registration unchanged; wire tenancy/authz
├── types/ | query/ | storage/          # object_collection streaming / durable object path as needed
├── release-profile/                    # FirstBinary still forbids s3 (unchanged)
└── conformance/
    └── tests/
        ├── dialect_s3_*.rs             # Suite A modules (ops, sigv4, semantics, isolation, limits)
        └── … existing compat_wire_* stay as HC smoke until replaced/extended

tests/
└── s3-interop/                         # Suite B (Docker) — not a Cargo member
    ├── docker-compose.yml
    ├── README.md
    ├── harness/                        # orchestrator: run 8 rows, SHA256, exit non-zero
    ├── clients/{rclone,awscli,boto3,s3fs,clickhouse-backup,elasticsearch,pgbackrest,restic}/
    └── matrix/                         # emits machine-readable row results

docs/ or specs/019…/artifacts/ (generated at implement)
└── s3-compatibility-matrix.{md,json}   # FR-057 / SC-008
```

**Structure Decision**: Expand `crates/handler-s3` in place (R1). Suite A lives in `crates/conformance`. Suite B is a sibling Docker tree under `tests/s3-interop/` so client images never build the server (FR-051). FirstBinary inventory in `release-profile` stays as-is (FR-013).

## Complexity Tracking

| Violation / tension | Why Needed | Simpler Alternative Rejected Because |
|---------------------|------------|--------------------------------------|
| Docker + Python boto3 in Suite B | Spec FR-051–FR-053 / SC-003 mandate eight unmodified clients including boto3 | Pure-Rust-only interop would not exercise rclone/s3fs/restic/pgBackRest; constitution I applies to the **server**, not client test images |
| `handler-s3` catalog/durable `execute_blocking` still runs on the serve task (`dispatch.rs`, not only `stream.rs`) | Existing `002`/`015` pattern | Implement MUST move those calls, including `dispatch.rs`, onto `spawn_blocking` / the storage durability pool (R8). This feature does not widen full-body buffering for multi-GB objects. Full async `QueryEngine` rewrite is out of scope |

No principle is waived. Docker Suite B is explicitly **not** a second server implementation.
