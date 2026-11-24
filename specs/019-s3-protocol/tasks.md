---
description: "Task list for complete-product S3 protocol dialect, conformance, and client interop"
---

# Tasks: S3 Protocol (Complete Product)

**Input**: Design documents from `/specs/019-s3-protocol/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Task count**: 94 tasks (T001–T087 plus insert IDs T010a, T010b, T016a, T016b, T018a, T023a, T025a)

**Tests**: Requested. Spec Independent Tests + DoD + FR-050–FR-057 + [suite-a.md](contracts/suite-a.md) + [suite-b.md](contracts/suite-b.md). **Write failing tests before implementation** for every listed test task. Mandatory cases MUST NOT rely exclusively on mocks.

**Scope of this feature**: Promote `crates/handler-s3` from HandlersComplete smoke to **complete-product S3 dialect**: full Supported operation set per [contracts/capability-matrix.md](contracts/capability-matrix.md), AWS SigV4 ([contracts/sigv4.md](contracts/sigv4.md)), namespace-bound authz (`014`), `object_collection` mapping (`003`/`002`), streaming I/O (no full-object RAM on hot path), durable multipart registry, expanded `compat::classify_s3`, `protocols.s3.*` config, Suite A in `crates/conformance`, Suite B under `tests/s3-interop/`, and published `s3-compatibility-matrix.{md,json}`. **FirstBinary unchanged** — `s3` stays in the forbidden set (`016` / FR-013).

**Sibling crates** (wire into; do not duplicate internals): `crates/handler-s3`, `crates/compat`, `crates/authz`, `crates/config`, `crates/node`, `crates/types`, `crates/query`, `crates/storage`, `crates/release-profile`, `crates/conformance`, `crates/protocol-core`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1]–[US10] on user-story phase tasks only
- Every task includes an exact file path

## Path Conventions

- Handler: `crates/handler-s3/src/`
- Conformance: `crates/conformance/tests/dialect_s3_*.rs`
- Interop: `tests/s3-interop/`
- Fixtures: `specs/019-s3-protocol/contracts/fixtures/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Crate layout, dependencies, fixture confs, and module stubs per [plan.md](plan.md) Project Structure

- [X] T001 Add workspace deps `quick-xml`, `md-5`, and CRC crates (`crc32fast` and/or `crc32c`) to `crates/handler-s3/Cargo.toml`; keep graph Rust-only (no `*-sys`)
- [X] T002 Create handler module tree stubs in `crates/handler-s3/src/` — `router.rs`, `etag.rs`, `checksum.rs`, `xml.rs`, `multipart.rs`, `stream.rs` — and `pub mod` them from `crates/handler-s3/src/lib.rs` without breaking existing HC build
- [X] T003 [P] Copy example shapes from [quickstart.md](quickstart.md) into contract fixture filenames per [contracts/fixtures/README.md](contracts/fixtures/README.md): `specs/019-s3-protocol/contracts/fixtures/s3-entrypoint.conf` (minimal `entrypoint s3` port, namespace-bound keys), `specs/019-s3-protocol/contracts/fixtures/s3-limits-raised.conf` (`limits.max_value` ≥ 1GiB for Suite B / large-object Suite A), `specs/019-s3-protocol/contracts/fixtures/s3-vhost.conf` (`protocols.s3 { region us-east-1; virtual_host_suffix … }`) — H3 / M7
- [X] T004 [P] Add `crates/handler-s3/README.md` documenting complete-product module map, streaming invariant (no full-object `Vec` on mandatory large path), and `cargo test -p spacestorage-handler-s3` supplement scope
- [X] T005 [P] Scaffold Suite B tree: `tests/s3-interop/docker-compose.yml`, `tests/s3-interop/README.md`, `tests/s3-interop/harness/`, `tests/s3-interop/clients/{rclone,awscli,boto3,s3fs,clickhouse-backup,elasticsearch,pgbackrest,restic}/`, `tests/s3-interop/matrix/` per [contracts/suite-b.md](contracts/suite-b.md)
- [X] T006 [P] Add conformance feature documentation in `crates/conformance/tests/README.md` (or extend existing): gate `cargo test -p spacestorage-conformance --features handlers-complete dialect_s3_`; document SC-007 peak-RSS rule (peak RSS MUST NOT grow linearly with object size on the large streaming fixture) and concurrent worker count **N = 8** for `dialect_s3_perf` (plan Performance Goals; A1)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Config knobs, compat matrix rows, authz/session seam, shared Suite A harness, FirstBinary guard tests. No user-story handler work until checkpoint.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

### Tests for Foundation ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T007 [P] Add unit tests in `crates/compat/src/matrix.rs` (`#[cfg(test)]`) asserting `classify_s3` for HandlersComplete returns `Must` for every verb in [contracts/capability-matrix.md](contracts/capability-matrix.md) Supported table (`HEADBUCKET`, `DELETEBUCKET`, `DELETEOBJECTS`, `COPYOBJECT`, `LISTOBJECTS`, `LISTPARTS`, `ABORTMULTIPARTUPLOAD`, `LISTMULTIPARTUPLOADS`, `GETBUCKETLOCATION`, `PRESIGNED` alias if classified) and `MustNot` for FirstBinary / `no_handler` profile
- [X] T008 [P] Add conformance in `crates/conformance/tests/release_profile_s3.rs`: `ReleaseProfile::FirstBinary` / `HandlerBuildSet` still lists `s3` as forbidden; `handlers_absent` invalid conf with `entrypoint s3` fails `entrypoint_unknown_handler` on `--features first-binary` (FR-013; no FirstBinary S3 listen)
- [X] T009 [P] Add config parse tests in `crates/config/tests/s3_protocol.rs` for `protocols.s3.virtual_host_suffix`, `region` (default `us-east-1`), `multipart_idle` (default `24h`), `multipart_min_part` (default `5MiB`), `multipart_max_parts` (default `10000`) per [contracts/config-directives.md](contracts/config-directives.md)
- [X] T010 [P] Add shared live-server harness `crates/conformance/tests/support/s3_live.rs` (or `dialect_s3_common.rs`): boot in-process or live-TCP node with namespace-bound SigV4 keys from `014` test fixtures; helper `sha256_record_and_verify` for every successful put/get pair (FR-042)
- [X] T010a [P] Add failing conformance cases in `crates/conformance/tests/dialect_s3_auth_foundation.rs` (or `support/s3_live.rs` module): principal with `namespace == None` → auth failure before `ListBuckets` or any data op — MUST fail before T014 lands (FR-006; H7)
- [X] T010b [P] Extend `dialect_s3_auth_foundation.rs`: `X-Amz-Security-Token` present/absent/wrong on temporary credentials → documented error before data access — MUST fail before T015 lands (FR-002; H7)

### Implementation for Foundation

- [X] T011 Expand `fn classify_s3` in `crates/compat/src/matrix.rs` so HandlersComplete/CompleteProduct `Must` verbs match [contracts/capability-matrix.md](contracts/capability-matrix.md) Supported rows; `MustNot` for `VERSIONING`, `OBJECTLOCK`, `REPLICATION`, `SELECT`, `ACL`, `LIFECYCLE`, `CORS`, `TAGGING`, `SSEKMS`, `SIGV2`, `UPLOADPARTCOPY`, `GETOBJECTATTRIBUTES` with stable codes per [contracts/errors.md](contracts/errors.md)
- [X] T012 Implement `protocols.s3 { … }` parsing and effective-config reporting in `crates/config/src/` per [contracts/config-directives.md](contracts/config-directives.md); reject duplicate S3 entrypoint addresses via existing `entrypoint_duplicate_address`
- [X] T013 Wire `virtual_host_suffix`, `region`, and multipart knobs from effective config into `crates/node/src/handler/s3.rs` and `crates/handler-s3/src/handler.rs` session context (read-only for dialect)
- [X] T014 Replace production-path demo auth (`x-spacestorage-demo-auth` / `DEMO_ACCESS_KEY`) with `014` access-key lookup in `crates/handler-s3/src/sigv4.rs` (or split `auth.rs`); refuse principals with `namespace == None` before `ListBuckets` or data ops (FR-006)
- [X] T015 Integrate session token verification (`X-Amz-Security-Token` header and presigned query) with `crates/authz/` principal store when temporary keys are issued (FR-002 clarification)
- [X] T016 Ensure `crates/handler-s3/src/dispatch.rs` (and/or `router.rs`) calls `compat::classify` **before** `LogicalRequest` / object ops for every routed verb; `MustNot` → S3 XML error from [contracts/errors.md](contracts/errors.md), never HTTP 200 empty success
- [X] T016a [P] Add failing conformance in `crates/conformance/tests/dialect_s3_query_options.rs`: every routed S3 verb with `X-SpaceStorage-Quorum` / `X-SpaceStorage-Timeout` (session header when set; `query_defaults` when unset) threads into session `QueryOptions` before `LogicalRequest`; unsatisfiable quorum → `QuorumUnsatisfiable` XML (FR-009; C1) — MUST fail before US1 dispatch rewrite
- [X] T016b Map `X-SpaceStorage-Quorum` and `X-SpaceStorage-Timeout` from request/session into `QueryOptions` on every S3 request in `crates/handler-s3/src/handler.rs` + `crates/handler-s3/src/dispatch.rs` before `LogicalRequest` / object ops; surface `QuorumUnsatisfiable` per [contracts/errors.md](contracts/errors.md) (FR-009; C1)
- [X] T017 Add `crates/conformance/Cargo.toml` dev-dependency on `aws-sdk-s3` (optional, Suite A presign smoke only) behind `handlers-complete` if needed — **supplement** to live HTTP, not a substitute for mandatory live-server cases (R11)

**Checkpoint**: `cargo test -p spacestorage-compat` classify_s3 tests pass. Config parses S3 directives. FirstBinary still forbids S3. Shared conformance harness compiles. T010a/T010b auth foundation tests failing until T014/T015. T016a/T016b green before Phase 3 US1 dispatch work (C1).

---

## Phase 3: User Story 1 — Core bucket and object API (Priority: P1) 🎯 MVP

**Goal**: Mandatory bucket/object operations: ListBuckets through ListMultipartUploads; path-style addressing; pagination; copy; batch delete; multipart lifecycle; empty bodies; GetBucketLocation constant region.

**Independent Test**: `cargo test -p spacestorage-conformance --features handlers-complete dialect_s3_ops` — each Supported op positive path with SHA256 round-trip ([spec.md](spec.md) US1).

### Tests for User Story 1 ⚠️

- [X] T018 [P] [US1] Add `crates/conformance/tests/dialect_s3_ops.rs` covering ListBuckets (namespace-scoped names), CreateBucket/HeadBucket/DeleteBucket (empty-only), GetBucketLocation (`us-east-1`), PutObject/HeadObject/GetObject/DeleteObject (**including zero-length** put/get), DeleteObjects quiet+non-quiet, CopyObject same bucket — each with SHA256 verify helper from `support/s3_live.rs`; duplicate `CreateBucket` → `BucketAlreadyOwnedByYou` / `BucketAlreadyExists`; delete non-empty bucket → `BucketNotEmpty` (M5)
- [X] T018a [P] [US1] Add Suite A cases to `dialect_s3_ops.rs`: response `Content-Length` conflicting with actual body → conformance failure per FR-033 (H5)
- [X] T019 [P] [US1] Extend `dialect_s3_ops.rs` with ListObjectsV2 (`prefix`, `delimiter`, `start-after`, `continuation-token`, `max-keys`, `encoding-type=url`) and ListObjects v1 (`marker`, `prefix`) consistency versus V2 on same fixture bucket; **exact `max-keys` pagination boundary** — no duplicate or missing keys across pages (H6)
- [X] T020 [P] [US1] Add `crates/conformance/tests/dialect_s3_multipart.rs`: CreateMultipartUpload → UploadPart → ListParts → CompleteMultipartUpload → GetObject (concatenated bytes SHA256); AbortMultipartUpload; ListMultipartUploads; negative part size below **5 MiB** (except last) → `EntityTooSmall`; wrong part order on complete → `InvalidPartOrder`; max **10000** parts boundary (M5)

### Implementation for User Story 1

- [X] T021 [P] [US1] Implement HTTP router in `crates/handler-s3/src/router.rs` mapping method/path/query to classified verbs per [contracts/operations.md](contracts/operations.md) (including service root `GET /` → ListBuckets, `DELETE /{bucket}` without key → DeleteBucket, `?list-type=2`, v1 list without list-type, `?delete`, `?uploads`, uploadId subresources)
- [X] T022 [US1] Refactor `CreateBucket` to create namespace-scoped `object_collection` container (default type `object_collection`) in `crates/handler-s3/src/dispatch.rs` — stop using `ensure_kv` / `K/V Store` as the primary bucket model (R5); honor `x-spacestorage-type` and `x-spacestorage-opt-*` for container/options without hiding inventory-reported types (M8)
- [X] T023 [P] [US1] Implement bucket ops (ListBuckets, HeadBucket, CreateBucket, DeleteBucket, GetBucketLocation) XML/HTTP responses in `crates/handler-s3/src/xml.rs` + dispatch; duplicate create → `BucketAlreadyOwnedByYou` / `BucketAlreadyExists`; non-empty delete → `BucketNotEmpty`
- [X] T023a [US1] Move catalog and durable `execute_blocking` waits in `crates/handler-s3/src/dispatch.rs` onto `spawn_blocking` / storage durability pool — MUST NOT block Tokio workers on quorum/fsync paths (constitution II, R8; H8) — complete before T024 object CRUD durable paths widen
- [X] T024 [P] [US1] Implement object CRUD (PutObject, GetObject, HeadObject, DeleteObject) with metadata capture and `object_collection` storage path via `crates/query` / `crates/storage` in `crates/handler-s3/src/dispatch.rs` and `crates/handler-s3/src/stream.rs` (chunked upload/download, no full-object buffer on streaming path)
- [X] T025a [P] [US1] Extend `dialect_s3_ops.rs` **before** CopyObject impl: `x-amz-metadata-directive` `COPY` vs `REPLACE`; CopyObject missing source key → `NoSuchKey`; `Cache-Control`, `Content-Disposition`, `Content-Encoding`, and `response-content-*` on GetObject round-trip / override expectations (H4)
- [X] T025 [US1] Implement DeleteObjects (batch XML) and CopyObject (`x-amz-copy-source`, metadata directive, response override headers, missing-key errors) in `crates/handler-s3/src/dispatch.rs` with same-namespace enforcement — after T025a tests fail
- [X] T026 [US1] Implement ListObjectsV2 and ListObjects v1 list XML in `crates/handler-s3/src/xml.rs` with pagination tokens and `encoding-type=url` key escaping
- [X] T027 [US1] Implement durable namespace-scoped multipart registry and ops in `crates/handler-s3/src/multipart.rs` (Create/UploadPart/ListParts/Complete/Abort/ListMultipartUploads) with idle deadline from `multipart_idle` config (default **24h**)
- [X] T028 [US1] Support path-style `/{bucket}/{key}` always; stub vhost branch in `crates/handler-s3/src/handler.rs` (full vhost in US3) — path-style MUST work for US1 checkpoint

**Checkpoint**: US1 acceptance scenarios 1–8 pass in `dialect_s3_ops` + `dialect_s3_multipart`.

---

## Phase 4: User Story 2 — AWS Signature Version 4 (Priority: P1)

**Goal**: Header auth, presigned URLs, payload hash modes (`UNSIGNED-PAYLOAD`, streaming payload modes), 15-minute skew, session token, SigV2/anonymous refusal.

**Independent Test**: `cargo test -p spacestorage-conformance --features handlers-complete dialect_s3_sigv4` ([spec.md](spec.md) US2).

### Tests for User Story 2 ⚠️

- [X] T029 [P] [US2] Add `crates/conformance/tests/dialect_s3_sigv4.rs`: valid `Authorization: AWS4-HMAC-SHA256` on mandatory ops; wrong signature → `SignatureDoesNotMatch`; wrong key → `InvalidAccessKeyId`; clock skew > **15 minutes** → `RequestTimeTooSkewed` (H1)
- [X] T030 [P] [US2] Extend `dialect_s3_sigv4.rs` with presigned URL within/after `X-Amz-Expires`; `x-amz-content-sha256: UNSIGNED-PAYLOAD` large PUT; multipart UploadPart per-request signing; **mandatory live Suite A** for `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` and `STREAMING-UNSIGNED-PAYLOAD-TRAILER` on PutObject/UploadPart before streaming impl (H2)
- [X] T031 [P] [US2] Add **mandatory live Suite A** in `dialect_s3_sigv4.rs` for canonical URI/query Unicode keys (spaces, `#?&`) on **path-style and virtual-hosted** (`s3-vhost.conf` fixture); unit-vector supplements in `crates/handler-s3/src/sigv4.rs` `#[cfg(test)]` are non-mandatory only (H3)
- [X] T032 [P] [US2] Extend `dialect_s3_sigv4.rs` for signed-request integration: SigV2 and anonymous requests → `InvalidArgument` / `AccessDenied` before data access; session-token cases build on foundation T010b

### Implementation for User Story 2

- [X] T033 [US2] Expand `crates/handler-s3/src/sigv4.rs` for full canonical request (URI encoding, query sorting, signed headers) per [contracts/sigv4.md](contracts/sigv4.md)
- [X] T034 [US2] Implement presigned query-string auth verification and expiry in `crates/handler-s3/src/sigv4.rs`
- [X] T035 [US2] Implement payload hash modes: literal SHA256 hex, `UNSIGNED-PAYLOAD`, `STREAMING-AWS4-HMAC-SHA256-PAYLOAD`, `STREAMING-UNSIGNED-PAYLOAD-TRAILER` in `crates/handler-s3/src/sigv4.rs` integrated with `crates/handler-s3/src/stream.rs`
- [X] T036 [US2] Reject SigV2 and unsigned requests (except documented presign) in `crates/handler-s3/src/handler.rs` auth gate; attach `x-amz-request-id` on authenticated responses

**Checkpoint**: US2 acceptance scenarios pass; no cross-namespace data on bad auth.

---

## Phase 5: User Story 3 — Protocol semantics and HTTP transport (Priority: P1)

**Goal**: Metadata round-trip, Range/conditionals, overwrite/concurrency, 100-continue, chunked encoding, keep-alive, interrupt single PUT, S3 XML errors, virtual-hosted when suffix configured.

**Independent Test**: `cargo test -p spacestorage-conformance --features handlers-complete dialect_s3_semantics` ([spec.md](spec.md) US3).

### Tests for User Story 3 ⚠️

- [X] T037 [P] [US3] Add `crates/conformance/tests/dialect_s3_semantics.rs` **before** T041 impl: `Content-Type` and `x-amz-meta-*` round-trip on Head/Get; `Cache-Control`, `Content-Disposition`, `Content-Encoding` on put and get; GetObject `response-content-*` override query params; Range `206` and invalid `416`; conditional `If-Match` / `If-None-Match` / `If-Modified-Since` / `If-Unmodified-Since` per supported subset (H4)
- [X] T038 [P] [US3] Extend semantics tests: concurrent PutObject same key (last writer wins); concurrent Delete+Put documented outcome; `Expect: 100-continue` on large PUT; chunked request/response bodies; HTTP/1.1 keep-alive multiple requests on one connection
- [X] T039 [P] [US3] Add interrupt test: drop connection mid single PUT → object not readable until successful retry; error XML shape `<Error><Code/><Message/><Resource/><RequestId/></Error>` for unsupported API sample

### Implementation for User Story 3

- [X] T040 [US3] Implement virtual-hosted routing in `crates/handler-s3/src/handler.rs` when `virtual_host_suffix` set; without suffix, path-style only and vhost Host MAY return documented `InvalidRequest` (R2)
- [X] T041 [US3] Implement conditional headers, Range responses, and response override headers (`Cache-Control`, `Content-Disposition`, `Content-Encoding`, `response-content-*`) in `crates/handler-s3/src/dispatch.rs` + `stream.rs` for GetObject/HeadObject/PutObject subset per [contracts/operations.md](contracts/operations.md) — after T037 tests (H4)
- [X] T042 [US3] Implement HTTP/1.1 keep-alive, `Expect: 100-continue`, and chunked `Transfer-Encoding` in `crates/handler-s3/src/handler.rs` without buffering full large bodies on the Tokio worker
- [X] T043 [US3] Implement last-writer-wins visibility and incomplete single-PUT non-publish rules in `crates/handler-s3/src/stream.rs` + storage commit path (durable ack only after complete body per `013`)
- [X] T044 [US3] Centralize S3 XML error renderer in `crates/handler-s3/src/xml.rs` mapping HTTP status + `Code` per [contracts/errors.md](contracts/errors.md); include quorum/timeout extension codes when `002` options headers set

**Checkpoint**: US3 acceptance scenarios pass including vhost when configured in fixture conf.

---

## Phase 6: User Story 4 — Data integrity (Priority: P1)

**Goal**: Content-MD5, `x-amz-checksum-sha256|crc32|crc32c`, ETag rules (single-part + multipart composite), SHA256 in all conformance writes.

**Independent Test**: Integrity sections green in `dialect_s3_ops`, `dialect_s3_multipart`, and shared SHA256 helper ([spec.md](spec.md) US4).

### Tests for User Story 4 ⚠️

- [X] T045 [P] [US4] Add integrity cases to `dialect_s3_ops.rs`: `Content-MD5` match → success, mismatch → `BadDigest`; `x-amz-checksum-sha256` / crc32 / crc32c match/mismatch named errors
- [X] T046 [P] [US4] Add ETag assertions in `dialect_s3_multipart.rs`: completed multipart ETag `"<md5-of-part-md5s>-<part-count>"` stable on HeadObject and ListObjectsV2; single-part content-derived hex documented in test comments (FR-020)

### Implementation for User Story 4

- [X] T047 [US4] Implement `crates/handler-s3/src/checksum.rs` validating Content-MD5 and supported `x-amz-checksum-*` on PutObject and UploadPart when present (CRC64NVME and trailer-only modes out of scope)
- [X] T048 [US4] Implement `crates/handler-s3/src/etag.rs` for single-part and composite multipart ETag; wire into Head/List/Get responses
- [X] T049 [US4] Ensure `support/s3_live.rs` records fixture SHA256 at write time and verifies on every successful read across all `dialect_s3_*` modules (FR-042)

**Checkpoint**: US4 acceptance scenarios pass; 0 silent checksum acceptances.

---

## Phase 7: User Story 5 — Namespace-bound security (Priority: P1)

**Goal**: Unbound credentials refused; two-namespace isolation; presigned and multipart id cannot cross namespaces; WRITE authz after valid signature.

**Independent Test**: `cargo test -p spacestorage-conformance --features handlers-complete dialect_s3_isolation` ([spec.md](spec.md) US5, FR-055).

### Tests for User Story 5 ⚠️

- [X] T050 [P] [US5] Add `crates/conformance/tests/dialect_s3_isolation.rs`: unbound-credential refusal builds on foundation T010a; two namespaces N1/N2 same bucket name — neither lists other's keys; presigned N1 used against N2 object → denied
- [X] T051 [P] [US5] Extend isolation: multipart upload id from N1 cannot UploadPart/Complete with N2 creds; READ-only principal → PutObject `AccessDenied` despite valid signature

### Implementation for User Story 5

- [X] T052 [US5] Enforce bucket/key authz via `014` closed vocabulary (CREATE/WRITE/READ/DROP) in `crates/handler-s3/src/dispatch.rs` after SigV4 success; namespace pinned for upload id registry lookups in `crates/handler-s3/src/multipart.rs`
- [X] T053 [US5] Bind SigV4 credential scope to single namespace for all ops including presigned URLs in `crates/handler-s3/src/sigv4.rs` + `crates/authz/` (FR-008)

**Checkpoint**: US5 acceptance scenarios pass; 0 cross-namespace reads/writes in isolation module.

---

## Phase 8: User Story 6 — S3 protocol conformance suite (Priority: P1)

**Goal**: Suite A modules complete with zero mandatory skips; negatives assert status + XML `Code`; pagination boundary; memory ceiling doc.

**Independent Test**: Full gate `cargo test -p spacestorage-conformance --features handlers-complete dialect_s3_` and complete-product profile when merge gate requires it ([contracts/suite-a.md](contracts/suite-a.md)).

### Tests for User Story 6 ⚠️

- [X] T054 [P] [US6] Add `crates/conformance/tests/dialect_s3_limits.rs`: `max_key` / `max_value` (default **16 MiB** assembled) → `limit_exceeded`; connection caps → connection refuse; `s3-limits-raised.conf` fixture allows Suite A large object write
- [X] T055 [P] [US6] Add `crates/conformance/tests/dialect_s3_unsupported.rs`: probe each **Unsupported by design** row in [contracts/capability-matrix.md](contracts/capability-matrix.md) — never silent HTTP 200; **Not yet implemented** `UploadPartCopy` / `GetObjectAttributes` → `NotImplemented`
- [X] T056 [P] [US6] Add `crates/conformance/tests/dialect_s3_perf.rs`: large-object streaming peak RSS below documented SC-007 ceiling (non-linear in object size); concurrent **N = 8** workers with SHA256; multipart idle timeout and **multipart throughput** smoke (M6)
- [X] T057 [US6] Add negative malformed cases to `dialect_s3_ops.rs` / `dialect_s3_multipart.rs`: invalid complete XML, wrong part numbers, unknown upload id → HTTP + `Code` per [contracts/errors.md](contracts/errors.md)

### Implementation for User Story 6

- [X] T058 [US6] Wire Suite A modules into `crates/conformance/Cargo.toml` (`handlers-complete` / complete-product features) with `#[ignore]` only for explicitly optional cases — **zero mandatory skips** for Supported ops
- [X] T059 [US6] Document Suite A module map and SC-007 peak-RSS ceiling (**N = 8** workers) in `crates/conformance/tests/README.md` aligning with [contracts/suite-a.md](contracts/suite-a.md) and T006; note relationship to existing `compat_handlers_complete.rs` / `compat_wire_handlers_complete.rs` S3 smokes (subset only until Suite A green)

**Checkpoint**: DoD items 1–2 and 5–7 satisfied for Suite A scope (US6).

---

## Phase 9: User Story 7 — Client interoperability harness (Priority: P1)

**Goal**: Docker Compose eight stock clients against external server; SHA256 verify; non-zero exit on any failure; no server build inside client images.

**Independent Test**: Single harness command from [quickstart.md](quickstart.md) §4 — eight rows pass ([contracts/suite-b.md](contracts/suite-b.md), FR-051–FR-056).

### Tests for User Story 7 ⚠️

- [X] T060 [P] [US7] Add harness dry-run test script `tests/s3-interop/harness/selftest.sh` (or Rust binary in `tests/s3-interop/harness/`) that fails fast when `S3_ENDPOINT` unset — connection error must not report pass (SC-003)
- [X] T061 [P] [US7] Add CI documentation job stub in `tests/s3-interop/README.md` describing shape: (1) start `spacestoraged` with `s3-limits-raised.conf`, (2) Suite A, (3) `docker compose` Suite B, (4) matrix generate — server **not** built in client Dockerfiles

### Implementation for User Story 7

- [X] T062 [P] [US7] Implement orchestrator in `tests/s3-interop/harness/` reading endpoint/region/keys/bucket/TLS from env; run **eight tool/client rows** plus **second SDK smoke row** (FR-053); emit JSON to `tests/s3-interop/matrix/results.json`; exit non-zero if **any** row fails (FR-056; M2)
- [X] T063 [P] [US7] Implement `tests/s3-interop/clients/rclone/` scenario: copy/sync + list + checksum verify vs fixture SHA256
- [X] T064 [P] [US7] Implement `tests/s3-interop/clients/awscli/` scenario: `aws s3` + `aws s3api` put/head/get/rm/cp + multipart where applicable
- [X] T065 [P] [US7] Implement `tests/s3-interop/clients/s3fs/` scenario: mount, write file, independent S3 get SHA256 verify
- [X] T066 [P] [US7] Implement `tests/s3-interop/clients/clickhouse-backup/` minimal artifact upload + list/get verify (Altinity path)
- [X] T067 [P] [US7] Implement `tests/s3-interop/clients/elasticsearch/` S3 snapshot repository register + minimal snapshot (+ companion ES service in `docker-compose.yml` if required)
- [X] T068 [P] [US7] Implement `tests/s3-interop/clients/pgbackrest/` repo-type S3 backup/check stores and verifies object (+ companion if required)
- [X] T069 [P] [US7] Implement `tests/s3-interop/clients/restic/` init/backup/check/restore small fixture with SHA256 verify

**Checkpoint**: All eight Suite B rows pass against reference deployment with raised limits.

---

## Phase 10: User Story 8 — AWS SDK interoperability (Priority: P1)

**Goal**: Dedicated boto3 scenarios + second SDK smoke in same CI gate (default Go v2 per research R13).

**Independent Test**: boto3 row + second SDK row in Suite B ([spec.md](spec.md) US8, FR-053).

### Tests for User Story 8 ⚠️

- [X] T070 [P] [US8] Add boto3 scenario Dockerfile + script in `tests/s3-interop/clients/boto3/`: `endpoint_url`, path-style put/get, virtual-hosted when suffix configured, `generate_presigned_url`, `upload_fileobj` multipart, metadata, Range `get_object`, unsupported API → `ClientError` with documented `Code`
- [X] T071 [P] [US8] Add second SDK smoke under `tests/s3-interop/clients/aws-sdk-go-v2/` **or** `tests/s3-interop/clients/aws-sdk-js/` — document choice in `tests/s3-interop/README.md`; minimal put/get against custom endpoint

### Implementation for User Story 8

- [X] T072 [US8] Wire boto3 and second SDK rows into `tests/s3-interop/harness/` aggregate pass/fail; orchestrator exit non-zero covers all **eight tool rows plus second SDK smoke** in one gate (M2)

**Checkpoint**: US8 acceptance scenarios pass; boto3 + second SDK documented in README.

---

## Phase 11: User Story 9 — Performance and resource behavior (Priority: P2)

**Goal**: Large-object streaming memory ceiling, concurrent sessions, multipart slow-client idle limit — conformance smokes not strict SLA.

**Independent Test**: `dialect_s3_perf.rs` green with documented ceiling ([spec.md](spec.md) US9, SC-007).

### Tests for User Story 9 ⚠️

- [X] T073 [P] [US9] Finalize `dialect_s3_perf.rs` with documented large fixture size from [contracts/fixtures/README.md](contracts/fixtures/README.md); measure peak RSS (or proxy metric) stays below ceiling independent of object size
- [X] T074 [P] [US9] Add concurrent **N = 8** upload/download workers test in `dialect_s3_perf.rs` with correct SHA256 and no FD exhaustion within suite timeout (A1)

### Implementation for User Story 9

- [X] T075 [US9] Audit `crates/handler-s3/src/dispatch.rs` and `crates/handler-s3/src/stream.rs` plus storage callbacks: all catalog/durable fsync/quorum waits use `spawn_blocking` / storage blocking pool only — never block Tokio worker on multi-MiB+ paths; complements T023a (constitution II, R8; H8)
- [X] T076 [US9] Enforce `multipart_idle` server-side cleanup job or lazy expiry in `crates/handler-s3/src/multipart.rs` so slow clients between parts do not hit premature abort before configured idle limit

**Checkpoint**: US9 acceptance scenarios pass; SC-007 mandatory large-object cases green.

---

## Phase 12: User Story 10 — Explicit unsupported surface (Priority: P2)

**Goal**: Matrix-accurate probes; ACL/versioning/select/replication return explicit errors; NYI placeholders stable.

**Independent Test**: `dialect_s3_unsupported.rs` + matrix audit ([spec.md](spec.md) US10, SC-002).

### Tests for User Story 10 ⚠️

- [X] T077 [P] [US10] Ensure `dialect_s3_unsupported.rs` covers versioning enable, object lock, replication, SelectObjectContent, object tagging, SSE-KMS header, canned ACL beyond authz mapping — each **Unsupported by design** per [contracts/capability-matrix.md](contracts/capability-matrix.md)

### Implementation for User Story 10

- [X] T078 [US10] Route unsupported bucket subresources and NYI ops in `crates/handler-s3/src/router.rs` to classified `MustNot` or explicit `NotImplemented` handlers before accidental CRUD fallback (extend 015 residual patterns in `handler.rs`)
- [X] T079 [US10] Document ignore-vs-reject behavior for ACL headers in `crates/handler-s3/src/dispatch.rs` per spec (no cross-tenant visibility)

**Checkpoint**: SC-002 — 0 silent successes on unsupported probes.

---

## Phase 13: Polish & Cross-Cutting Concerns

**Purpose**: Compatibility matrix artifact, observability labels, release gating, quickstart validation, complete-product claim

- [X] T080 [P] Implement matrix generator `scripts/s3-matrix-gen.rs` consuming [contracts/capability-matrix.md](contracts/capability-matrix.md) and Suite A/B JSON results → output `docs/s3-compatibility-matrix.md` and `docs/s3-compatibility-matrix.json` (FR-057, SC-008; M3)
- [X] T081 [P] Add automated matrix audit test in `crates/conformance/tests/s3_matrix_audit.rs` (complete-product feature): every Supported row has `suite_a: true` in generated JSON when Suite A green
- [X] T082 Ensure S3 request observability uses `protocol="s3"`, `kind=<operation>` in `crates/handler-s3/src/handler.rs` per intent `008` — additive only, no label renames
- [X] T083 Enforce `limits.max_key` / `limits.max_value` (assembled multipart sum ≤ **max_value** default **16 MiB** unless raised) and connection caps on S3 entrypoint in `crates/handler-s3/src/dispatch.rs` + `crates/node/` accept path — `limit_exceeded{limit,current,max}` not hang (FR-012)
- [X] T084 [P] Align [quickstart.md](quickstart.md) with implemented commands and fixture paths; runbook for local Suite A + B under 60 minutes when Docker available
- [X] T085 Run FirstBinary regression: `cargo test -p spacestorage-conformance --test release_profile_s3 --test handlers_absent` — there is no `first-binary` Cargo feature; these tests confirm S3 stays forbidden and `entrypoint s3` fails `entrypoint_unknown_handler`. Suite A is not required on that profile (FR-013)
- [X] T086 Run complete-product / handlers-complete merge gate: `cargo test -p spacestorage-conformance --features handlers-complete` with each `--test dialect_s3_*` target (a bare `dialect_s3_` filter matches function names and runs nothing). All green before milestone claim
- [X] T087 Copy generated matrix into `specs/019-s3-protocol/artifacts/s3-compatibility-matrix.{md,json}` on release tagging workflow (document path in `tests/s3-interop/README.md`)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 — **BLOCKS** all user stories; **T016a/T016b (FR-009) MUST complete before US1 dispatch rewrite (T021+)**; **T010a before T014**, **T010b before T015**
- **User Stories (Phases 3–12)**: All depend on Phase 2 completion
  - Recommended sequence: US1 → US2 → US3 → US4 → US5 (handler core), then US6 (Suite A consolidation), US7–US8 (Suite B), US9–US10 (P2)
  - US2 (SigV4) can overlap US1 only after T016 classify wire — SigV4 tests (T029+) need minimal PutObject from US1 or harness mock server; prefer US1 router skeleton before full SigV4 streaming modes
- **Polish (Phase 13)**: Depends on US6–US8 green for matrix generator

### User Story Dependencies

| Story | Depends on | Independent test |
|-------|------------|-------------------|
| US1 | Phase 2 | `dialect_s3_ops`, `dialect_s3_multipart` |
| US2 | Phase 2, US1 minimal routing | `dialect_s3_sigv4` |
| US3 | US1–US2 | `dialect_s3_semantics` |
| US4 | US1 multipart | checksum/ETag in ops + multipart modules |
| US5 | US2 authz, US1 data | `dialect_s3_isolation` |
| US6 | US1–US5 modules exist | full `dialect_s3_*` gate |
| US7 | US1–US4 on live server | `tests/s3-interop` harness |
| US8 | US7 harness | boto3 + second SDK rows |
| US9 | US1 streaming | `dialect_s3_perf` |
| US10 | Phase 2 classify | `dialect_s3_unsupported` |

### Parallel Opportunities

- Phase 1 tasks T003–T006 marked [P]
- Phase 2 tests T007–T010, T010a, T010b [P] before T011–T017; T016a before T016b; T016b before Phase 3
- Within each story, tests marked [P] before implementation block
- Suite B client directories T063–T069 [P] after T062 orchestrator skeleton
- Polish T080–T081 [P] after Suite A/B JSON exists

### Parallel Example: User Story 7

```bash
# After T062 harness skeleton, implement client rows in parallel:
tests/s3-interop/clients/rclone/
tests/s3-interop/clients/awscli/
tests/s3-interop/clients/restic/
# … remaining six client trees
```

---

## Implementation Strategy

### MVP First (User Story 1 + Foundation)

1. Complete Phase 1–2 (config, classify, authz, harness)
2. Complete Phase 3 US1 (core ops + multipart) with tests T018–T020 **first**
3. **STOP and VALIDATE**: `dialect_s3_ops` + `dialect_s3_multipart` on `s3-limits-raised.conf`
4. Add US2 SigV4, then US3–US5 before claiming dialect complete

### Incremental Delivery

1. Foundation → US1 ops → US2 auth → US3 HTTP → US4 integrity → US5 isolation
2. US6 Suite A full gate → US7–US8 Docker interop → US9–US10 P2
3. Polish matrix + FirstBinary regression before complete-product S3 claim

### Parallel Team Strategy

- Developer A: handler-s3 US1–US3 implementation after foundation
- Developer B: conformance `dialect_s3_*` tests (TDD ahead of A)
- Developer C: Suite B Docker tree after US1 live server available
- Developer D: compat/config/authz foundation (Phase 2)

---

## Notes

- Do **not** change `016` FirstBinary forbidden handler set or claim FirstBinary S3 support
- Wire version remains SigV4 per `015` / `002`; handler smoke promotion ≠ new protocol port
- Every successful put/get in Suites A and B MUST use SHA256 verify helper (FR-042)
- Multipart: intermediate parts ≥ **5 MiB** except last; max **10000** parts; idle **24h** default
- Suite B client images MUST NOT compile `spacestoraged` (FR-051)
- Promote **Not yet implemented** ops only via spec/matrix change — tasks assume `UploadPartCopy` / `GetObjectAttributes` stay NYI

---

## Phase 14: Convergence

Gaps the open checkboxes do not already name. Streaming, keep-alive / 100-continue / interrupted PUT, limits, SHA256-on-every-write, error `Resource`, Suite B, matrix, and polish stay on the existing `[ ]` tasks (T024, T038, T039, T042–T044, T049, T054, T056–T087).

- [X] T088 CRITICAL: Gate `x-spacestorage-demo-auth` in `crates/handler-s3/src/handler.rs` `authenticate` so non-test builds cannot authenticate with an access-key id and no SigV4 signature; keep a test-only path for Suite A per research R4 (contradicts) per FR-002
- [X] T089 Return the stored ETag header on PutObject and UploadPart in `crates/handler-s3/src/handler.rs` (UploadPart must not answer with a stub `<ETag>` XML body), put the stored ETag in `CopyObjectResult` instead of the literal `copied` in `crates/handler-s3/src/dispatch.rs`, and send `x-amz-id-2` with `x-amz-request-id` per [contracts/operations.md](contracts/operations.md) (partial) per FR-020
- [X] T090 Parse the CompleteMultipartUpload part-list XML in `crates/handler-s3/src/xml.rs` and `crates/handler-s3/src/dispatch.rs`, assemble only the listed parts in listed order, return `CompleteMultipartUploadResult`, and reject bad ETags or order with `InvalidPart` / `InvalidPartOrder` per [contracts/operations.md](contracts/operations.md) and [contracts/errors.md](contracts/errors.md) (partial) per FR-003
- [X] T091 Honor PutObject `If-None-Match: *` (do not overwrite an existing key) in `crates/handler-s3/src/handler.rs` per [contracts/operations.md](contracts/operations.md) (missing) per FR-022
- [X] T092 Make `ListMultipartUploads` and `DeleteBucket` emptiness in `crates/handler-s3/src/dispatch.rs` observe uploads persisted in the durable registry, not only the in-memory map, so a memory drop or restart still lists in-progress uploads and blocks delete (partial) per research R7
- [X] T093 Add Suite A cases in `crates/conformance/tests/dialect_s3_multipart.rs` for AbortMultipartUpload, ListMultipartUploads, and the configured max-parts ceiling (`InvalidRequest`; default 10000) that T020 marked done without those cases (partial) per FR-050

---

## Phase 15: Convergence

Gaps the open checkboxes do not already name. Streaming, keep-alive / 100-continue / interrupted PUT, error `Resource`, SHA256-on-every-write, limits, `block_in_place` durability waits, Suite B, matrix, and polish stay on the existing `[ ]` tasks (T024, T038, T039, T042–T044, T049, T054, T056–T087).

- [X] T094 Accept UploadPart bodies below `multipart_min_part` and return `EntityTooSmall` only when a non-final part in the CompleteMultipartUpload list is under 5 MiB, in `crates/handler-s3/src/handler.rs` and `crates/handler-s3/src/dispatch.rs`; retarget `entity_too_small_intermediate_part` so a small part that may be last is accepted and a non-final undersized part fails at complete per FR-026 (partial)
- [X] T095 Reject CreateBucket names outside `^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$` with `InvalidBucketName`, and expose non-conforming container names as `x--<hex>` on ListBuckets, in `crates/handler-s3/src/dispatch.rs` and `crates/handler-s3/src/xml.rs` per plan: data-model bucket naming (missing)

---

## Phase 16: Convergence

Checked-off Suite A/B work stands. These gaps are still in the handler.

- [X] T096 CRITICAL: Return success for `CreateMultipartUpload` and `UploadPart` only after `persist_upload` in `crates/handler-s3/src/dispatch.rs` commits the durable registry write; propagate `exec` failures instead of discarding them with `let _ = session.exec` so an acknowledgement is not issued for an in-memory part alone per Constitution II (contradicts)
- [X] T097 Keep multipart part bodies out of the in-memory registry in `crates/handler-s3/src/multipart.rs` and `crates/handler-s3/src/dispatch.rs`: stream each `UploadPart` to durable storage in `CHUNK_WINDOW` pieces and retain only part number, ETag, and size; `CompleteMultipartUpload` must publish the object from those stored pieces without `Vec::with_capacity(total)` or cloning every `PartRecord.bytes` (research R6) per FR-032 (contradicts)
- [X] T098 Pump `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` and `STREAMING-UNSIGNED-PAYLOAD-TRAILER` PutObject bodies through the chunk-window path in `crates/handler-s3/src/handler.rs` (verify each chunk signature as it arrives) instead of `read_request_body` plus `decode_aws_chunked` of the entire payload per FR-032 (partial)
- [X] T099 Fail `ListObjects` and `ListObjectsV2` with `QuorumUnsatisfiable` when the session quorum cannot be met, and do not turn `Point`/`exec` errors into zero-size keys, in `user_keys` / `stored_object_stat` in `crates/handler-s3/src/dispatch.rs` per FR-009 (partial)

---

## Phase 17: Convergence

Phase 16 holds: multipart registry metadata, streaming SigV4 `PutObject`, and list quorum. These handler paths still violate the streaming and fail-closed rules. `UploadPartCopy` / `GetObjectAttributes` stay **Not yet implemented**, and **Unsupported by design** stays on the MustNot probes.

- [X] T100 Stop `apply_get_modifiers` in `crates/handler-s3/src/handler.rs` from allocating `vec![0; content_length]` when `HEADOBJECT` on a streamed manifest returns `S3Reply::Head`; derive range and `Content-Length` from the stored length so `HeadObject` does not require the full object in memory per FR-032 (partial)
- [X] T101 Pump `UploadPart` through the chunk-window path in `crates/handler-s3/src/handler.rs` instead of `read_request_body` when `uploadId` is set (`should_stream_put` currently returns false); hash Content-MD5, part ETag, and checksums incrementally and write each window in `upload_part_bytes` without holding the whole part per FR-032 (partial)
- [X] T102 Make `object_key_exists` in `crates/handler-s3/src/dispatch.rs` return the `exec` error instead of `false`, and fail `PutObject` `If-None-Match: *` in `crates/handler-s3/src/handler.rs` on that error so a quorum or point failure cannot overwrite an existing key per FR-009 (partial)
- [X] T103 Return the `exec` error from `DeleteObject` in `crates/handler-s3/src/dispatch.rs` instead of `let _ = session.exec` followed by `204 No Content`, so a failed durable delete is not acknowledged per FR-003 (partial)

---

## Phase 18: Convergence

Phase 17 holds: HeadObject does not allocate the object, UploadPart streams by window, `If-None-Match: *` fails closed, and `DeleteObject` returns `exec` errors. `UploadPartCopy` / `GetObjectAttributes` stay **Not yet implemented**. **Unsupported by design** stays on the MustNot probes. These supported paths still acknowledge a failed durable mutation, or turn a failed multipart read into an empty success / `NoSuchUpload`.

- [X] T104 Return the `exec` error from `DeleteBucket` in `crates/handler-s3/src/dispatch.rs` instead of `let _ = session.exec` on `TypeOp` `drop` followed by `204 No Content`, so a failed durable drop is not acknowledged per FR-003 (partial)
- [X] T105 Stop `DeleteObjects` in `crates/handler-s3/src/dispatch.rs` from discarding each `DeleteObject` reply and listing every key under `Deleted`; surface that failure (missing bucket or `exec` error) in the `DeleteResult` instead of a success body per FR-003 (partial)
- [X] T106 Return the `exec` error from `AbortMultipartUpload` when `delete_persisted_upload` in `crates/handler-s3/src/dispatch.rs` fails, instead of `204 No Content` after the discarded deletes, so the upload is not reported aborted while the durable registry row remains per FR-003 (partial)
- [X] T107 Propagate `user_keys` and `Point` errors from `list_persisted_uploads` and `load_upload` in `crates/handler-s3/src/dispatch.rs` on `ListMultipartUploads`, `ListParts`, `UploadPart`, `CompleteMultipartUpload`, and `AbortMultipartUpload`, instead of an empty upload list or `NoSuchUpload`, per FR-009 (partial)

---

## Phase 19: Convergence

Phase 18 holds: `DeleteBucket`, `DeleteObjects`, and `AbortMultipartUpload` return durable `exec` errors, and multipart list/load does not turn `Point` errors into an empty list or `NoSuchUpload`. `UploadPartCopy` / `GetObjectAttributes` stay **Not yet implemented**. **Unsupported by design** stays on the MustNot probes. Staging-chunk and superseded-part deletes after a successful publish stay residual cleanup.

- [X] T108 Fail `GetObject` and `HeadObject` in `crates/handler-s3/src/dispatch.rs` with `QuorumUnsatisfiable` via `require_read_quorum` before `LogicalRequest::Point`, so an unsatisfiable session read quorum does not return object bytes or metadata the way `ListObjects` and `ListParts` already refuse per FR-009 (partial)