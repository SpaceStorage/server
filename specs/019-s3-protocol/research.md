# Research: S3 Protocol (019)

Phase 0 decisions for `specs/019-s3-protocol`. All Technical Context unknowns from planning are resolved here. Spec clarifications (0 questions after expansion) are taken as given.

## R1 — Crate layout: expand `handler-s3` vs new crate

- **Decision**: Expand `crates/handler-s3` in place; keep `crates/node/src/handler/s3.rs` as the thin port adapter.
- **Rationale**: HC smoke, `WIRE_VERSION`, release-profile registration, and conformance already target this crate. A second S3 crate would fork the dialect.
- **Alternatives considered**: New `handler-s3-cp` crate — rejected (duplicate listen path, dual classify). Rewrite on axum router only — deferred; keep tokio HTTP/1.1 loop for streaming control, optionally share parsers with `protocol-core`.

## R2 — Addressing: path-style and virtual-hosted

- **Decision**: Path-style `/{bucket}/{key}` always. Virtual-hosted `{bucket}.<suffix>` when `protocols.s3.virtual_host_suffix` is set and `Host` matches; without suffix, path-style works and vhost MAY return documented `InvalidRequest` / `InvalidArgument`.
- **Rationale**: Spec Clarifications + FR-034; matches `002` s3 contract.
- **Alternatives considered**: Vhost-only — rejected (AWS CLI path-style common with custom endpoints). Always-on default suffix — rejected (breaks IP-literal endpoints).

## R3 — SigV4 surface

- **Decision**: Implement header auth, query (presigned) auth, clock skew ≤ 15 min → `RequestTimeTooSkewed`, reject SigV2 and anonymous. Payload modes: literal hex SHA256, `UNSIGNED-PAYLOAD`, `STREAMING-AWS4-HMAC-SHA256-PAYLOAD`, `STREAMING-UNSIGNED-PAYLOAD-TRAILER`. Session token via `X-Amz-Security-Token` / `X-Amz-Security-Token` query when `014` issues temporary keys.
- **Rationale**: Spec User Story 2 + Clarifications; required by matrix tools.
- **Alternatives considered**: Header-only SigV4 — rejected (presigned + UNSIGNED used by boto3/rclone). STS as separate product — rejected (honor token when present; no STS API required).

## R4 — Authz integration

- **Decision**: Map SigV4 access key → `014` principal; secret from principal store (not `DEMO_ACCESS_KEY`). Unbound principals (including unbound `CLUSTER_ADMIN`) refused at authentication on S3. Authorize CREATE/WRITE/READ/DROP via closed vocabulary / implicit tenant grant. Replace smoke bypass `x-spacestorage-demo-auth` with test-only feature or remove from non-test builds.
- **Rationale**: FR-006–FR-008, `014` FR-002 / Story 1.
- **Alternatives considered**: Keep demo keys in production path — rejected. Unbound admin header to select namespace — rejected by constitution/014.

## R5 — Bucket / object type mapping

- **Decision**: `CreateBucket` creates container with default type `object_collection` in schema `public`; honor `x-spacestorage-type` / `x-spacestorage-opt-*` when present. Keys map to object bytes + metadata (Content-Type, `x-amz-meta-*`, ETag, Last-Modified). Stop using `ensure_kv` / `L3Model::KvStore` as the S3 bucket model for Supported ops.
- **Rationale**: FR-010; `002` s3 contract native mapping.
- **Alternatives considered**: Keep KvStore for S3 — rejected (wrong model; blocks metadata/ETag). Per-key containers — rejected (bucket is the container).

## R6 — Streaming and memory

- **Decision**: Upload/download pump bodies in chunks through `stream.rs` into storage ObjectOps (or equivalent append API). Incomplete single-PUT MUST NOT publish a readable object. Multipart parts stored durably under upload id; complete assembles without holding full object in the handler. Peak RSS ceiling documented in Suite A README (default: object size must not appear as a linear term in measured peak for the large fixture).
- **Rationale**: FR-032, SC-007, constitution II.
- **Alternatives considered**: Buffer full body in `Vec<u8>` (current smoke) — rejected for Supported large path. Spill only to tmpfile then rename without datatype path — rejected (bypasses quorum/durability).

## R7 — Multipart registry

- **Decision**: Namespace-scoped multipart upload registry (upload id → bucket, key, initiated time, parts metadata) durable enough for ListMultipartUploads / ListParts across connections and process restart per datatype durability tier. Idle abort after configurable `protocols.s3.multipart_idle` (document default, e.g. 7 days AWS-like or shorter CP default — **default 24h** for conformance predictability).
- **Rationale**: Spec scenarios 5–6; Restic/pgBackRest resume; FR-026.
- **Alternatives considered**: Per-session `HashMap` (current) — rejected (broken across connections). Always-ephemeral RAM registry — rejected for durable buckets.

## R8 — Blocking / durability

- **Decision**: Async read/write of sockets and chunk staging. Any fsync or durable-quorum wait runs on the dedicated blocking pool (`spawn_blocking` / storage pool). Catalog metadata mutations that currently use `execute_blocking` MUST be wrapped so they do not occupy the Tokio worker during durable wait. Handler MUST NOT call durable ack on in-memory-only write for persistent/hybrid containers.
- **Rationale**: Constitution II; intent `13`; FR-009.
- **Alternatives considered**: Leave full-body `execute_blocking` on serve task — rejected for large objects. Full rewrite of `PlannerEngine` to async-only — out of scope; track as shared debt.

## R9 — `compat::classify_s3` expansion

- **Decision**: Mark every **Supported and tested** verb as `Must` for HandlersComplete and CompleteProduct profiles (FirstBinary stays `no_handler` → MustNot). **Unsupported by design** → MustNot with stable codes (`VERSIONING`, `OBJECTLOCK`, …). **Not yet implemented** (`UPLOADPARTCOPY`, `GETOBJECTATTRIBUTES`) → MustNot / handler returns `NotImplemented` until promoted. This feature owns the S3 rows for matrix generation (FR-011) and updates `015` matrix docs on merge if needed.
- **Rationale**: Current classify only lists a subset; ListBuckets root, DeleteBucket, CopyObject, DeleteObjects, ListObjects v1, ListParts, Abort, ListMultipartUploads missing as Must.
- **Alternatives considered**: Handler-only routing without classify — rejected (`015` requires classify before IR).

## R10 — Limits vs multi-GB backups

- **Decision**: Keep `015` defaults (`max_value` 16 MiB assembled object). Suite A large-object and Suite B backup tools use a **raised-limits** fixture (`limits.max_value` and related caps). Document in quickstart that production backup namespaces MUST raise limits. Multipart min part **5 MiB** (except last); max parts **10 000**.
- **Rationale**: Spec Assumptions; `015` limits contract; avoids silently changing FB/HC defaults.
- **Alternatives considered**: Raise global default to 5 GiB — rejected (memory/admission surprise). Per-protocol unlimited S3 — rejected (must still enforce named errors).

## R11 — Suite A layout

- **Decision**: New modules under `crates/conformance/tests/` (e.g. `dialect_s3_ops.rs`, `dialect_s3_sigv4.rs`, `dialect_s3_semantics.rs`, `dialect_s3_isolation.rs`, `dialect_s3_limits.rs`, `dialect_s3_unsupported.rs`) gated by `handlers-complete` (and run under complete-product CI). Live server only for mandatory cases; unit tests in `handler-s3` for pure SigV4 vectors allowed as supplements, not substitutes.
- **Rationale**: FR-050; mirrors `dialect_redis` / `dialect_pg` pattern; 017-style capability coverage without waiting on 017 plan.
- **Alternatives considered**: External pytest-only Suite A — rejected (merge gate is cargo conformance). Mock-only — rejected by DoD.

## R12 — Suite B Docker harness

- **Decision**: `tests/s3-interop/` compose stack: orchestrator + eight client scenarios. Server is external (CI starts `spacestoraged` or testcontainer separately). Each row: write → read → SHA256 verify → relevant list/multipart/interrupt. Exit non-zero if any row fails. Companion containers for Elasticsearch and pgBackRest as needed.
- **Rationale**: FR-051–FR-056, User Story 7.
- **Alternatives considered**: Embed server build in client Dockerfile — rejected (FR-051). Single mega-image with all tools — acceptable alternative if compose still isolates rows; prefer per-tool services for clear fail messages.

## R13 — boto3 + second SDK

- **Decision**: boto3 scenarios in Suite B (path-style, virtual, endpoint_url, region, presign, multipart, metadata, range, unsupported error Code). Second SDK: **AWS CLI** already counts as matrix tool; add **aws-sdk-go-v2** smoke **or** `@aws-sdk/client-s3` Node smoke as an explicit Suite A/B sibling job documented in suite-b contract (implementer picks one; default **Go v2** for static binary in Alpine).
- **Rationale**: FR-053 / User Story 8.
- **Alternatives considered**: boto3 only — fails FR-053 “at least one additional”. Rust `aws-sdk-s3` only — weaker interop signal for operators.

## R14 — Compatibility matrix artifact

- **Decision**: Source of truth table in [contracts/capability-matrix.md](contracts/capability-matrix.md) (+ JSON twin generated at implement). Generator merges static statuses with Suite A/B pass marks → published `s3-compatibility-matrix.{md,json}` for complete-product docs. Statuses: Supported and tested | Unsupported by design | Not yet implemented.
- **Rationale**: FR-011, FR-057, SC-008.
- **Alternatives considered**: Hand-maintained wiki only — rejected (drift). Derive solely from classify enum — insufficient for client-tool rows.

## R15 — ETag and checksums

- **Decision**: Single-part ETag = content-derived hex (MD5 of body when feasible; document if weak-ETag form used). Multipart complete = `"<md5-of-part-md5s>-<part-count>"`. Validate Content-MD5 and `x-amz-checksum-sha256|crc32|crc32c` when sent. CRC64NVME and trailer-only modes out of scope.
- **Rationale**: Spec Clarifications + FR-020 / FR-040–041.
- **Alternatives considered**: Always SHA256 as ETag — rejected (breaks AWS CLI expectations). Skip Content-MD5 — rejected (BadDigest required).

## R16 — Feature / profile gating (first binary unchanged)

- **Decision**: `ReleaseProfile::FirstBinary` continues to forbid handler name `s3`. No change to `016` obligations. Deep dialect + Suite A/B are required for HandlersComplete wire maturity and CompleteProduct merge claims. Conformance feature flags: extend existing `handlers-complete`; Suite B is a separate CI job not linked into FirstBinary tests.
- **Rationale**: FR-013; release-profile already encodes forbidden set.
- **Alternatives considered**: New Cargo feature `s3-dialect` on FirstBinary — rejected (would ship S3). Gate S3 only behind CompleteProduct — rejected (HC already requires s3 handler presence; this feature fills the dialect).

## Resolved Technical Context checklist

| Item | Resolution |
|------|------------|
| Language | Rust 1.88 / edition 2024 |
| HTTP stack | Existing tokio HTTP/1.1 in handler-s3 (+ quick-xml) |
| Auth | SigV4 → 014 |
| Storage model | object_collection + durable multipart registry |
| Streaming | chunk pump; no full-object RAM |
| Suites | A = conformance Rust; B = Docker |
| Limits | 015 defaults; fixtures raise for large |
| First binary | unchanged / s3 forbidden |
