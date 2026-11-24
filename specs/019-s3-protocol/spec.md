# Feature Specification: S3 Protocol (Complete Product)

**Feature Branch**: `019-s3-protocol`

**Created**: 2026-11-23

**Status**: Approved (review-spec and review-plan passed; human approved)

**Input**: Production-quality S3-compatible object storage for the complete product: explicit compatibility profile (supported operations, limitations, protocol semantics), AWS Signature Version 4, two independent conformance suites (protocol + eight-tool Docker interop), namespace-bound authz, and a three-status compatibility matrix. **Not** full AWS S3 parity. S3 is **not** in the first binary (`016`).

## Clarifications

Record of specification decisions encoded during this expansion (no open `[NEEDS CLARIFICATION]` markers).

| Topic | Decision |
| --- | --- |
| AWS S3 scope | **Compatibility profile**, not parity: only operations in the mandatory set and matrix-driven extras are required; everything else is **Unsupported by design** or **Not yet implemented** with documented errors. |
| Addressing | **Path-style** (`/{bucket}/{key}`) and **virtual-hosted-style** (`{bucket}.<configured-suffix>`) both MUST work when the entrypoint declares a virtual-host suffix; without it, path-style MUST work and vhost MAY be refused with a documented error. |
| ListObjects | **ListObjectsV2** is mandatory (intent `15`). **ListObjects** (v1, `marker`) is mandatory because backup tools and older clients still emit it; behavior aligns with V2 where parameters overlap. |
| Checksums | **Content-MD5** MUST be verified when the client sends it. **Supported**: `x-amz-checksum-sha256`, `x-amz-checksum-crc32`, `x-amz-checksum-crc32c` on PutObject and UploadPart where sent. **Unsupported by design**: AWS SDK checksum algorithm **CRC64NVME** and trailer checksum modes not required for the eight matrix tools (see matrix). |
| ETag | ETag is **not** always raw object MD5: single-part uploads use documented content-derived hex; completed multipart uploads use the AWS **composite** form (`"<md5-of-part-md5s>-<part-count>"`); copying and multipart edge cases follow documented rules in FR-semantic block. |
| Session tokens | **AWS STS session token** (`X-Amz-Security-Token` header or presigned query parameter) MUST be honored when present on namespace-bound credentials that issue temporary keys; invalid or cross-namespace tokens MUST fail authentication. |
| SigV4 clock skew | Reject with `RequestTimeTooSkewed` when skew exceeds **15 minutes** (AWS-compatible default). |
| Streaming payload signing | **UNSIGNED-PAYLOAD**, `STREAMING-AWS4-HMAC-SHA256-PAYLOAD`, and `STREAMING-UNSIGNED-PAYLOAD-TRAILER` MUST be supported for large uploads where matrix clients use them. |

## User Scenarios & Testing *(mandatory)*

The first binary does **not** include an S3 handler (intent `16`). This feature defines the **complete-product S3 dialect**, an **S3 capability table** with three statuses, and **two independent test suites** against a live server (no mock-only coverage for mandatory behavior).

### User Story 1 — Core bucket and object API (Priority: P1)

An operator or application uses stock S3 clients (AWS CLI, boto3, rclone, backup tools) against the SpaceStorage S3 entrypoint. The client signs with SigV4, manages buckets and objects using the **mandatory operation set**, lists with pagination, copies and batch-deletes where supported, and completes multipart uploads for large objects.

**Why this priority**: CRUD, listing, copy, batch delete, and multipart are the foundation of the intent `15` ceiling and every matrix integration.

**Independent Test**: Execute the **S3 protocol conformance suite** mandatory-operation scenarios for one namespace-bound credential; each successful write is read back and **SHA256** of body is compared to the fixture.

**Acceptance Scenarios**:

1. **Given** a running S3 entrypoint and namespace-bound credentials, **When** the client calls `ListBuckets`, **Then** only buckets in that namespace appear and names carry no namespace prefix.
2. **Given** an authenticated session, **When** the client `CreateBucket`, `HeadBucket`, `PutObject`, `HeadObject`, `GetObject`, `CopyObject` (same bucket), and `DeleteObject` on a key, **Then** each step returns the documented success form, `GetObject` body equals `PutObject` body, and `HeadObject` metadata matches `PutObject`.
3. **Given** a bucket with multiple keys, **When** the client `DeleteObjects` (quiet and non-quiet) and `ListObjectsV2` with `prefix`, `delimiter`, `start-after`, `continuation-token`, and `encoding-type=url`, **Then** results match layout and pagination behaves like AWS for the supported parameter subset.
4. **Given** `ListObjects` (v1) with `marker` and `prefix`, **When** listing the same bucket as in scenario 3, **Then** key sets are consistent with V2 for the same effective filters.
5. **Given** an object larger than the documented multipart threshold, **When** the client `CreateMultipartUpload`, `UploadPart`, `ListParts`, `CompleteMultipartUpload`, and `GetObject`, **Then** the object bytes match the concatenated parts and `ListObjectsV2` reports correct size and ETag per documented rules.
6. **Given** an in-progress multipart upload, **When** the client `AbortMultipartUpload` and `ListMultipartUploads`, **Then** no completed object exists for that upload id and in-progress state is accurate.
7. **Given** empty buckets and empty object bodies, **When** the client `PutObject` with zero-length body and `GetObject`, **Then** success is returned and the retrieved body is empty with correct metadata.
8. **Given** `GetBucketLocation`, **When** called on an existing bucket, **Then** the response returns the documented region constant (default `us-east-1` for compatibility) without implying real AWS regions.

---

### User Story 2 — AWS Signature Version 4 (Priority: P1)

Clients authenticate with AWS SigV4 using header auth, presigned URLs, and payload hashing modes that matrix tools require. Invalid, expired, or skewed signatures never reach object data.

**Why this priority**: Without interoperable SigV4, none of the eight integrations connect.

**Independent Test**: Protocol conformance **SigV4** scenarios (positive, negative, presigned, multipart signing) run without mocks.

**Acceptance Scenarios**:

1. **Given** valid `Authorization: AWS4-HMAC-SHA256` with correct canonical request, **When** any mandatory operation is invoked, **Then** authentication succeeds and `x-amz-request-id` is present on the response.
2. **Given** a presigned URL within `X-Amz-Expires`, **When** the client performs the signed HTTP method, **Then** the operation succeeds; **When** the URL is used after expiry, **Then** `AccessDenied` or signature error is returned.
3. **Given** `x-amz-content-sha256: UNSIGNED-PAYLOAD` on a signed PUT, **When** uploading a large object, **Then** the upload succeeds without requiring a precomputed body hash in the signature.
4. **Given** object keys with Unicode, spaces, and `#?&` characters, **When** canonical URI and query encoding are applied per SigV4 rules, **Then** PUT and GET succeed for the same key via path-style and vhost-style addressing.
5. **Given** a multipart upload, **When** each `UploadPart` is signed with the correct payload hash, **Then** parts are accepted and `CompleteMultipartUpload` succeeds.
6. **Given** a deliberately wrong signature or wrong secret, **When** any request is sent, **Then** `SignatureDoesNotMatch` or `InvalidAccessKeyId` is returned and no data is returned from another namespace.
7. **Given** request time outside the 15-minute skew window, **When** a signed request arrives, **Then** `RequestTimeTooSkewed` is returned.
8. **Given** a temporary credential with `X-Amz-Security-Token`, **When** the token is valid for the principal, **Then** operations succeed; **When** the token is omitted or wrong, **Then** authentication fails.
9. **Given** SigV2 or anonymous requests (no exception documented), **When** submitted, **Then** authentication is refused (`InvalidArgument` / `AccessDenied` per documented mapping) before data access.

---

### User Story 3 — Protocol semantics and HTTP transport (Priority: P1)

Clients rely on AWS-shaped metadata, conditional requests, ranges, overwrites, concurrency behavior, and HTTP/1.1 streaming. The server does not load entire large objects into memory for streaming paths.

**Why this priority**: Backup and mount tools depend on ranges, metadata, keep-alive, and chunked uploads.

**Independent Test**: Conformance suite scenarios for semantics, HTTP behavior, and resource limits on a live server.

**Acceptance Scenarios**:

1. **Given** `PutObject` with `Content-Type` and `x-amz-meta-*` headers, **When** `HeadObject` and `GetObject` are called, **Then** returned HTTP headers reflect stored metadata.
2. **Given** a stored object, **When** `GetObject` is called with `Range`, **Then** `206 Partial Content` with correct byte range is returned; invalid range yields `416` where applicable.
3. **Given** conditional headers, **When** `If-Match` / `If-None-Match` / `If-Modified-Since` / `If-Unmodified-Since` are sent per AWS rules for supported ops, **Then** `304 Not Modified` or `412 Precondition Failed` match documented conditions.
4. **Given** concurrent `PutObject` to the same key, **When** both complete, **Then** one object version is visible (last successful write wins per documented atomicity); concurrent `DeleteObject` and `PutObject` yield documented outcomes without corrupt bodies.
5. **Given** `Expect: 100-continue` on a large PUT, **When** the client waits for `100 Continue`, **Then** the server responds per HTTP/1.1 expectations before consuming the body.
6. **Given** chunked `Transfer-Encoding` uploads and downloads, **When** bodies stream, **Then** correct content is stored and returned with valid `Content-Length` or chunked encoding on responses as appropriate.
7. **Given** HTTP/1.1 keep-alive, **When** multiple S3 requests reuse a connection, **Then** all complete without requiring one request per connection.
8. **Given** an interrupted upload mid-body, **When** the connection drops, **Then** no complete object appears unless the client used atomic single PUT or completed multipart; partial single PUT does not publish a readable object.
9. **Given** errors for unsupported APIs, **When** returned, **Then** the body is S3 XML `<Error><Code/><Message/><Resource/><RequestId/></Error>` with stable `Code` values from the capability table.

---

### User Story 4 — Data integrity (Priority: P1)

Integrators and conformance tests verify checksums, MD5, and ETag rules so silent corruption is detectable.

**Why this priority**: Backup products (Restic, pgBackRest, clickhouse-backup) assume integrity checks.

**Independent Test**: For every successful write in conformance and interop suites, **SHA256** of stored bytes is compared after read; negative checksum tests assert correct errors.

**Acceptance Scenarios**:

1. **Given** `Content-MD5` on `PutObject`, **When** the digest matches the body, **Then** the upload succeeds; **When** it does not match, **Then** `BadDigest` or documented equivalent is returned.
2. **Given** `x-amz-checksum-sha256` (or crc32/crc32c) on put or upload part, **When** the checksum matches, **Then** success; **When** it does not, **Then** a named checksum error is returned.
3. **Given** a completed multipart object, **When** ETag is compared to documented composite rules, **Then** it is stable across `HeadObject` and `ListObjectsV2`.
4. **Given** download after upload, **When** the client computes SHA256 of the response body, **Then** it matches the fixture SHA256 recorded at upload time in the test.

---

### User Story 5 — Namespace-bound security (Priority: P1)

Tenancy from intent `14` applies: credentials without namespace binding are refused; cross-namespace access via signatures, presigned URLs, or multipart upload ids is impossible.

**Why this priority**: Object storage conflating tenants is a critical data-leak class.

**Independent Test**: Dedicated conformance **isolation** scenarios with two namespaces and two credential pairs.

**Acceptance Scenarios**:

1. **Given** credentials with no namespace binding, **When** any S3 request is made, **Then** authentication fails before `ListBuckets` or data operations.
2. **Given** two namespaces N1 and N2 with the same bucket name, **When** each credential lists and writes, **Then** neither session sees the other's buckets or keys.
3. **Given** a presigned URL issued for N1, **When** used against an object in N2 (same key name), **Then** access is denied.
4. **Given** a multipart upload id created in N1, **When** `UploadPart` or `CompleteMultipartUpload` is attempted with N2 credentials using that id, **Then** the request fails with a documented upload/bucket error.
5. **Given** WRITE permission denied on a bucket, **When** `PutObject` is attempted with a valid signature, **Then** `AccessDenied` is returned (signature alone does not bypass authz).

---

### User Story 6 — S3 protocol conformance suite (Priority: P1)

A maintainer runs **Suite A**: independent tests for every **Supported and tested** API operation—positive, negative, malformed, boundary, concurrency, pagination, and failure cases—against a running SpaceStorage deployment.

**Why this priority**: Proves the dialect without conflating coverage with any single client.

**Independent Test**: `cargo test` (or documented equivalent) for the S3 conformance profile executes Suite A with **zero** mandatory skips and **zero** mock-only mandatory cases.

**Acceptance Scenarios**:

1. **Given** Suite A configuration (endpoint, credentials, buckets), **When** the suite runs, **Then** each mandatory operation has at least one dedicated test file or module and results are pass/fail per operation.
2. **Given** negative cases (invalid XML, wrong part numbers, unknown upload id), **When** executed, **Then** HTTP status and XML `Code` match the capability table.
3. **Given** pagination boundary (exactly `max-keys` objects), **When** `ListObjectsV2` continues with token, **Then** no duplicate or missing keys versus ground truth inserted by the test.

---

### User Story 7 — Client interoperability harness (Priority: P1)

A maintainer or CI job runs **Suite B**: a **Docker** image or compose stack that exercises **all eight** matrix tools against the same live endpoint, covering each tool's relevant operations (write, read, SHA256 verify, overwrite, delete, list, multipart where used, recovery after interrupt where the tool supports it).

**Why this priority**: Product promise is unmodified real-world clients, not only in-house HTTP tests.

**Independent Test**: One harness run produces eight rows (pass/fail) and exits non-zero on any failure.

**Acceptance Scenarios**:

1. **Given** harness config (endpoint, region/custom endpoint, keys, TLS trust, bucket), **When** the harness starts, **Then** it does not build the SpaceStorage server inside client images; it targets an external running server.
2. **Given** **rclone**, **When** copy/sync and list with checksum verification run, **Then** the row passes.
3. **Given** **AWS CLI** (`aws s3` and `aws s3api`), **When** put, head, get, rm, cp, and multipart where applicable run, **Then** the row passes.
4. **Given** **boto3** (see User Story 8), **When** included in harness or a dedicated sibling job invoked from the same CI gate, **Then** boto3 row passes.
5. **Given** **s3fs**, **When** mount, write file, read via independent S3 get, **Then** the row passes.
6. **Given** **Altinity clickhouse-backup** S3 upload path, **When** minimal artifact upload and list/get verification run, **Then** the row passes.
7. **Given** **Elasticsearch** S3 snapshot repository, **When** repository registration and minimal snapshot run, **Then** the row passes.
8. **Given** **pgBackRest** repo-type S3, **When** documented backup/check stores and verifies an object, **Then** the row passes.
9. **Given** **Restic**, **When** init, backup, check, and restore of a small fixture run, **Then** the row passes.

---

### User Story 8 — AWS SDK interoperability (Priority: P1)

Python **boto3** and at least **one additional AWS SDK** (e.g. AWS CLI already in matrix, plus **aws-sdk-go-v2** or **@aws-sdk/client-s3** in conformance documentation) exercise endpoint configuration patterns operators use in production.

**Why this priority**: boto3 is the reference SDK for custom endpoints and presigned URLs.

**Independent Test**: Dedicated boto3 scenario module in Suite B (or linked suite) covering path-style, virtual-hosted, custom endpoint URL, region, presigned URL, multipart, metadata, range GET, and error parsing.

**Acceptance Scenarios**:

1. **Given** `endpoint_url` pointing at SpaceStorage, **When** boto3 client uses path-style addressing, **Then** put/get succeed.
2. **Given** virtual-hosted suffix configured on the server, **When** boto3 uses `addressing_style='virtual'`, **Then** put/get succeed.
3. **Given** `generate_presigned_url`, **When** used before expiry, **Then** GET or PUT succeeds per method signed.
4. **Given** `upload_fileobj` / multipart transfer, **When** uploading a file above threshold, **Then** completed object SHA256 matches source file.
5. **Given** `get_object` with `Range`, **When** requesting bytes, **Then** partial content matches.
6. **Given** a call to an **Unsupported by design** API, **When** boto3 parses the error, **Then** `ClientError` exposes the documented `Code`.

---

### User Story 9 — Performance and resource behavior (Priority: P2)

Operators need assurance that large objects and concurrent sessions do not require full-object RAM buffering or unbounded file descriptors.

**Why this priority**: Mount and backup workloads stream multi-GB objects.

**Independent Test**: Conformance **performance** scenarios (not necessarily strict SLA): large object stream, concurrent uploads, interrupted connection, multipart throughput smoke, memory ceiling check documented in test README.

**Acceptance Scenarios**:

1. **Given** an object at or above the documented large-object fixture size, **When** uploaded and downloaded with streaming clients, **Then** the server's **peak memory** during the test stays below the documented conformance ceiling (independent of object size).
2. **Given** N concurrent upload/download workers (documented N), **When** the load test runs, **Then** all complete with correct SHA256 and without FD exhaustion errors.
3. **Given** a long-running multipart upload, **When** the client slows between parts, **Then** the server does not time out before the documented multipart idle limit.

---

### User Story 10 — Explicit unsupported surface (Priority: P2)

Clients probing AWS features outside the product ceiling receive predictable errors; the **compatibility matrix** is published and accurate.

**Why this priority**: Prevents false parity claims and silent data loss.

**Independent Test**: Matrix rows marked **Unsupported by design** are probed in Suite A; each returns not-supported/not-implemented, never silent success.

**Acceptance Scenarios**:

1. **Given** a bucket, **When** versioning, object lock, or replication configuration APIs are invoked, **Then** not-supported/not-implemented per intent `15`.
2. **Given** an object, **When** `SelectObjectContent` is invoked, **Then** not-supported.
3. **Given** canned ACL / grant headers beyond SpaceStorage authz mapping, **When** submitted, **Then** rejection or documented ignore behavior without cross-tenant visibility.
4. **Given** an operation marked **Not yet implemented**, **When** invoked before implementation ships, **Then** `NotImplemented` (or equivalent) names the operation; matrix is updated when status changes to **Supported and tested**.

---

### Edge Cases

- Duplicate `CreateBucket` in the same namespace: `BucketAlreadyOwnedByYou` / `BucketAlreadyExists` per AWS naming rules.
- `DeleteBucket` non-empty: `BucketNotEmpty` until all objects and in-progress multipart uploads are removed (per documented rules).
- Part size below 5 MiB (except last part): `EntityTooSmall`.
- More than documented max parts per upload: `InvalidRequest` / `InvalidPart`.
- Wrong `partNumber` order on complete: `InvalidPartOrder`.
- `CopyObject` from missing source key: `NoSuchKey`.
- `encoding-type=url` on list: XML-escaped keys round-trip for special characters.
- Transfer interrupted during single PUT: object not readable until retry succeeds.
- Harness cannot reach endpoint: row fails with connection error, not pass.
- Elasticsearch / pgBackRest rows: companion containers in compose; missing dependency fails that row with explicit message.
- Credential valid on PostgreSQL but not authorized for S3: `AccessDenied` at S3 authz, not mixed-protocol bleed.

## S3 compatibility profile *(mandatory for this feature)*

This section is the **S3 capability table** owned by FR-011. It extends intent `15` and aligns with `specs/002-protocol-drivers/contracts/protocols/s3.md` without replacing that contract wholesale. Status values:

| Status | Meaning |
| --- | --- |
| **Supported and tested** | MUST work; Suite A has dedicated tests; appears in published matrix. |
| **Unsupported by design** | MUST return explicit not-supported/not-implemented; Suite A probes per User Story 10. |
| **Not yet implemented** | MUST return predictable `NotImplemented` (or documented placeholder) until promoted; matrix MUST list it. |

### Mandatory bucket and object operations (complete-product)

| Operation | HTTP / API shape (summary) | Status | Notes |
| --- | --- | --- | --- |
| ListBuckets | `GET /` | Supported and tested | Namespace-scoped; optional `SpaceStorageType` extension in XML ignored by stock clients. |
| HeadBucket | `HEAD /{bucket}` | Supported and tested | Used by CLI and mount tools. |
| CreateBucket | `PUT /{bucket}` | Supported and tested | `CreateBucketConfiguration` accepted; `LocationConstraint` ignored. |
| DeleteBucket | `DELETE /{bucket}` | Supported and tested | Empty bucket only. |
| GetBucketLocation | `GET /{bucket}?location` | Supported and tested | Returns documented constant region. |
| PutObject | `PUT /{bucket}/{key}` | Supported and tested | Metadata, conditional `If-None-Match: *`, checksum headers. |
| GetObject | `GET /{bucket}/{key}` | Supported and tested | Range, conditionals, `response-*` overrides subset per 002 contract. |
| HeadObject | `HEAD /{bucket}/{key}` | Supported and tested | |
| DeleteObject | `DELETE /{bucket}/{key}` | Supported and tested | |
| DeleteObjects | `POST /{bucket}?delete` | Supported and tested | Quiet mode supported; required by AWS CLI batch deletes. |
| CopyObject | `PUT /{bucket}/{key}` + `x-amz-copy-source` | Supported and tested | Required by rclone and AWS CLI `cp`. |
| ListObjectsV2 | `GET /{bucket}?list-type=2` | Supported and tested | `prefix`, `delimiter`, `continuation-token`, `start-after`, `max-keys`, `encoding-type=url`. |
| ListObjects | `GET /{bucket}` (v1) | Supported and tested | `marker`, `prefix`, `delimiter`, `max-keys`. |
| CreateMultipartUpload | `POST /{bucket}/{key}?uploads` | Supported and tested | |
| UploadPart | `PUT ?uploadId&partNumber` | Supported and tested | Max parts and min part size per limits (`015`). |
| CompleteMultipartUpload | `POST ?uploadId` | Supported and tested | |
| AbortMultipartUpload | `DELETE ?uploadId` | Supported and tested | |
| ListParts | `GET ?uploadId` | Supported and tested | Multipart resume (Restic, pgBackRest, ES). |
| ListMultipartUploads | `GET /{bucket}?uploads` | Supported and tested | Cleanup and tooling. |

### Operations required by matrix clients (beyond intent `15` minimum)

| Operation | Required by | Status |
| --- | --- | --- |
| HeadBucket | AWS CLI, s3fs, boto3 | Supported and tested |
| HeadObject | AWS CLI, rclone | Supported and tested |
| DeleteObjects | AWS CLI | Supported and tested |
| CopyObject | rclone, AWS CLI | Supported and tested |
| GetBucketLocation | Some SDK startup checks | Supported and tested |
| ListObjects (v1) | Legacy list clients | Supported and tested |
| Multipart full lifecycle | All large-backup tools | Supported and tested |
| Presigned URL (query SigV4) | boto3, browser-style GET | Supported and tested |

### Unsupported by design (intent `15` MUST NOT and product non-goals)

| Capability | Status | Error expectation |
| --- | --- | --- |
| Bucket versioning (enable/suspend) | Unsupported by design | `NotImplemented` / `501` on enable |
| Object lock / legal hold | Unsupported by design | Not supported |
| S3 replication configuration | Unsupported by design | Not supported |
| S3 Select (`SelectObjectContent`) | Unsupported by design | Not supported |
| ACL / canned ACL beyond authz mapping | Unsupported by design | Reject or documented ignore |
| Bucket lifecycle configuration | Unsupported by design | Not supported |
| Bucket CORS / website / policy XML writes | Unsupported by design | Not supported |
| Server-side encryption product (`x-amz-server-side-encryption` as AWS KMS) | Unsupported by design | `501` until product encryption mapping (`07`/`14`) says otherwise |
| Object tagging API | Unsupported by design | `501 NotImplemented` |
| SigV2 | Unsupported by design | `InvalidArgument` |
| Batch operations (S3 Control / Jobs) | Unsupported by design | Out of AWS S3 data-plane scope; `NotImplemented` / `501` |

### Not yet implemented (explicit placeholder — promote via spec change)

| Capability | Status | Notes |
| --- | --- | --- |
| UploadPartCopy | Not yet implemented | Optional for some sync tools; return `NotImplemented` until built. |
| GetObjectAttributes | Not yet implemented | Optional; return `NotImplemented` until built. |

Published **compatibility matrix** artifact (human-readable + machine-readable) MUST be generated from this table and Suite A/B results and MUST ship with the complete-product profile.

## Requirements *(mandatory)*

### Functional Requirements — handler and tenancy

- **FR-001**: The complete product MUST expose an S3 protocol handler on its own entrypoint port (protocol-per-port, intent `02`); it MUST NOT share a listen address with another protocol handler.
- **FR-002**: The S3 handler MUST authenticate with **AWS Signature Version 4** for the supported operation set: header-based auth, query-string (presigned) auth, `x-amz-content-sha256` including `UNSIGNED-PAYLOAD`, canonical URI and query-string encoding (including Unicode keys), multipart per-request signing, session token header/query when issued, rejection of expired and invalid signatures, and `RequestTimeTooSkewed` beyond 15-minute skew.
- **FR-003**: The handler MUST implement every operation marked **Supported and tested** in the compatibility profile table.
- **FR-004**: Operations marked **Unsupported by design** MUST return explicit not-supported or not-implemented responses per intent `15` and the table; **never** silent empty success.
- **FR-005**: Operations marked **Not yet implemented** MUST return predictable documented errors until promoted to **Supported and tested** and the matrix is updated.
- **FR-006**: The handler MUST refuse credentials with **no namespace binding** at authentication (intent `14`); all data operations MUST scope to exactly one namespace per session.
- **FR-007**: Bucket names MUST be unique per namespace, not cluster-wide; the same display name MAY exist in two namespaces with isolation.
- **FR-008**: Invalid signatures, presigned URLs, and multipart upload ids MUST NOT bypass namespace or bucket authz checks.
- **FR-009**: Each request MUST honor session or per-operation quorum and timeout through the documented option mechanism (`02`), with administrator defaults when unset.
- **FR-010**: S3 buckets and objects MUST map to the shared datatype model (`02`/`003`) with default `object_collection` on bucket create; documented type options MUST remain available without hiding inventory types.
- **FR-011**: This feature MUST own the **S3 capability table** and **compatibility matrix** (three statuses); where this spec adds S3 detail beyond `015`, this spec governs for S3 implementation and conformance merge.
- **FR-012**: Documented size, connection, and quota limits (`015`, `007`) MUST be enforced with named S3 errors, not hang or silent truncate.
- **FR-013**: First-binary builds MAY omit the S3 handler; `016` MUST remain unchanged.

### Functional Requirements — protocol semantics

- **FR-020**: **ETag** values MUST follow documented rules: content-derived for single-part PUT; composite form for completed multipart; stable across Head/List for a given object version; clients MUST NOT assume ETag is always raw MD5 of the whole object.
- **FR-021**: User-defined metadata (`x-amz-meta-*`), `Content-Type`, and standard HTTP cache/disposition headers on PUT MUST round-trip on Head/Get within supported subset.
- **FR-022**: **Range** GET and conditional requests (`If-Match`, `If-None-Match`, `If-Modified-Since`, `If-Unmodified-Since`) MUST behave per AWS rules for supported operations.
- **FR-023**: Object overwrite (sequential PUT to same key) and concurrent PUT/DELETE MUST yield documented visibility (atomic published object per successful complete write or multipart complete).
- **FR-024**: Empty objects (zero-length body) MUST be supported.
- **FR-025**: Object keys MUST be binary-safe UTF-8 with percent-encoding in list XML when `encoding-type=url`.
- **FR-026**: Multipart uploads MUST enforce minimum part size (5 MiB except last part unless limits doc differs), maximum part count, and lifecycle cleanup for aborted uploads per documented idle timeout.
- **FR-027**: Error responses MUST use AWS-compatible XML error bodies with `Code`, `Message`, `Resource`, and `RequestId`; extension codes (e.g. quorum) MUST remain documented in the matrix.

### Functional Requirements — HTTP and streaming

- **FR-030**: HTTP/1.1 **keep-alive** MUST be supported for sequential requests on one connection.
- **FR-031**: **Chunked transfer encoding** and **Expect: 100-continue** MUST be supported for applicable uploads.
- **FR-032**: Large upload and download paths MUST stream without requiring the full object size in memory (constitution-compatible durability MAY still use blocking pool for fsync).
- **FR-033**: Responses MUST use correct `Content-Length` or chunked encoding; conflicting length and body MUST fail conformance tests.
- **FR-034**: **Path-style** addressing MUST be supported; **virtual-hosted-style** MUST be supported when the entrypoint declares a virtual-host suffix (see Clarifications).

### Functional Requirements — data integrity

- **FR-040**: When `Content-MD5` is sent, the server MUST validate before accepting the object.
- **FR-041**: Supported AWS checksum headers (`x-amz-checksum-sha256`, `crc32`, `crc32c`) MUST be validated on put/upload part when present.
- **FR-042**: Conformance and interop suites MUST record **SHA256** of every fixture at write time and verify on read for successful paths.
- **FR-043**: Multipart complete MUST produce integrity-consistent objects verified by SHA256 of full body in tests.

### Functional Requirements — testing architecture

- **FR-050**: **Suite A (S3 protocol conformance)** MUST test every **Supported and tested** operation independently against a live server with positive, negative, malformed, boundary, concurrency, pagination, and failure cases; mandatory cases MUST NOT rely exclusively on mocks.
- **FR-051**: **Suite B (client interoperability)** MUST ship as Docker image or compose orchestrating stock client images only; it MUST NOT embed building the SpaceStorage server in client images.
- **FR-052**: Suite B MUST run scenarios for **rclone**, **AWS CLI**, **clickhouse-backup**, **Elasticsearch S3 repository**, **s3fs**, **boto3**, **pgBackRest**, and **Restic**, each covering that client's relevant ops (write, read, SHA256 verify, overwrite, delete, list, multipart where used, interrupt recovery where supported).
- **FR-053**: A **dedicated boto3** scenario set MUST cover endpoint config, addressing styles, custom endpoint URL, region, presigned URLs, multipart, metadata, range, and error handling; at least one additional AWS SDK MUST have a documented smoke scenario in the same gate.
- **FR-054**: **Performance/resource** conformance scenarios MUST cover large-object streaming memory ceiling, concurrent transfers, multipart throughput smoke, interrupted connections, and long-running upload idle behavior.
- **FR-055**: **Namespace isolation** tests MUST cover unbound credentials, cross-namespace bucket/key names, presigned cross-tenant attempts, and cross-namespace multipart id reuse.
- **FR-056**: Suite A and B MUST emit structured pass/fail suitable for CI; Suite B MUST exit non-zero if any matrix row fails.
- **FR-057**: Compatibility **documentation** MUST be generated so each operation's status (**Supported and tested** / **Unsupported by design** / **Not yet implemented**) matches implementation and test results.

### Functional Requirements — harness configuration

- **FR-060**: The harness MUST accept endpoint URL, credentials, bucket (or auto-create), TLS trust, and custom-endpoint/region settings required by matrix tools.
- **FR-061**: Harness documentation MUST describe prerequisites (server version, namespace-bound credential with WRITE, optional companion services) and a single command for full matrix locally or in CI.

### Definition of Done (feature complete)

Feature work on S3 for the complete-product profile is **done** only when all of the following hold:

1. Every **Supported and tested** operation has Suite A coverage (mandatory scenarios).
2. Every mandatory protocol semantics requirement (FR-020–FR-027, FR-030–FR-034, FR-040–FR-043) has Suite A tests.
3. All **eight** client integrations pass Suite B in one CI-equivalent run.
4. Every successful write in Suites A and B is verified by read-back **SHA256** comparison.
5. Negative tests assert correct HTTP status and XML `Code`.
6. Namespace isolation scenarios (FR-055) pass.
7. No mandatory test relies exclusively on mocks.
8. Unsupported features are listed in the matrix; probes for **Unsupported by design** pass User Story 10.
9. The published compatibility matrix accurately reflects implementation and test status.

### Key Entities *(include if feature involves data)*

- **S3 Compatibility Matrix**: Authoritative three-status list of operations and parameters; owned by this feature; consumed by implementation, docs, and conformance.
- **S3 Session**: SigV4-authenticated session bound to one principal, one namespace, transport mode, optional quorum/timeout options.
- **Bucket (S3 view)**: Container in the bound namespace; S3 bucket name without namespace prefix.
- **Object key**: Key within a bucket mapping to bytes and metadata (ETag, size, last modified, user metadata).
- **Multipart upload**: In-progress upload identified by upload id and parts; namespace-scoped.
- **Suite A Run / Suite B Run**: Executions of protocol conformance or Docker interop with configuration snapshot, logs, and aggregate pass/fail.
- **Matrix Tool Row**: One of eight tool ids with scenario description, pass/fail, and failure excerpt.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: **100%** of User Stories **1–5** acceptance scenarios pass in **Suite A** against a reference deployment; **100%** of **User Story 8** acceptance scenarios pass in **Suite B** (not Suite A); **0** mandatory failures attributable to those stories in the respective suite.
- **SC-002**: **100%** of **Unsupported by design** probes in User Story 10 return explicit errors; **0** silent successes.
- **SC-003**: **100%** of Suite B matrix rows (eight tools) report **pass** in one harness run with verified writes; **0** false passes.
- **SC-004**: **100%** of documented put/get pairs in Suites A and B (empty, sub-multipart, and multipart sizes) match on **SHA256**; **0** mismatches.
- **SC-005**: **100%** of unbound-credential authentication attempts fail before data access; **0** cross-namespace reads or writes in isolation tests.
- **SC-006**: **100%** of limit-exceeded cases return named errors within the suite timeout; **0** indefinite hangs in mandatory conformance cases.
- **SC-007**: Large-object streaming scenarios complete with peak memory below the documented conformance ceiling for **100%** of mandatory large-object cases.
- **SC-008**: Published compatibility matrix matches implementation status for **100%** of listed operations at release tag (audit via automated matrix check or manual gate checklist).

## Assumptions

- **Complete-product scope.** S3 is not in the first binary (`016`); PostgreSQL/Redis first-binary obligations are unchanged.
- **Not AWS parity.** The profile is explicit; gaps are documented in the matrix, not implied.
- **002 contract.** Wire details in `specs/002-protocol-drivers/contracts/protocols/s3.md` inform this spec; FR-011 makes this feature authoritative for complete-product S3 dialect and tests.
- **Region.** Any SigV4 region string MAY be accepted with documented default `us-east-1` in location responses for client compatibility.
- **Multipart threshold.** Clients commonly use 5 MiB minimum intermediate part size and 8–64 MiB thresholds to start multipart; exact server limits are documented with `015` limits at plan time.
- **Harness target.** Suites assume a running SpaceStorage deployment; CI starts the server separately then runs A and B.
- **Companion containers.** Elasticsearch and pgBackRest rows may use compose dependencies; not production topology.
- **Credentials.** Tests use admin/role surfaces (`07`/`14`) to mint namespace-bound keys with bucket WRITE.
- **Checksum algorithms.** See Clarifications; CRC64NVME and trailer-only modes are out of scope unless a future matrix tool requires them.
- **ListObjects v1.** Retained for older clients; new integrators should prefer V2.

## Dependencies

- Protocol handlers, entrypoints, namespace rules (`specs/002-protocol-drivers`).
- Compatibility ceiling and limits (`specs/015-compatibility-and-limits`).
- Authz, namespace binding, TLS (`specs/014-authz-keys`, `specs/007`).
- Shared types and object storage mapping (`specs/003`).
- Query execution (`specs/005`).
- Durability and quorum (`specs/013`, `specs/012`, `specs/004`).
- MVP sequencing (`specs/016-mvp-and-nongoals`).

## Out of Scope

- First-binary delivery of any S3 handler.
- Full AWS S3 parity and S3 Control / Batch Operations planes.
- AWS features in the `015` S3 MUST NOT column (versioning, object lock, replication, S3 Select, ACL product beyond SpaceStorage authz).
- Hosting ClickHouse, Elasticsearch, or PostgreSQL as SpaceStorage protocols—only their **S3 client** usage in Suite B.
- Billing, invoicing, or multi-process `spacestoraged` orchestration beyond existing runtime specs.
- Replacing SigV4 with a custom auth scheme.