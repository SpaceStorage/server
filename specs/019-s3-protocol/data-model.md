# Data Model: S3 Protocol (019)

Entities for the complete-product S3 dialect. Wire shapes: [contracts/operations.md](contracts/operations.md). Auth: [contracts/sigv4.md](contracts/sigv4.md). Capability statuses: [contracts/capability-matrix.md](contracts/capability-matrix.md).

## 1. S3 Compatibility Matrix

Authoritative three-status inventory of operations and selected parameters.

| Field | Type | Notes |
|-------|------|-------|
| `operation_id` | string | e.g. `PutObject`, `ListObjectsV2` |
| `status` | enum | `supported_tested` \| `unsupported_by_design` \| `not_yet_implemented` |
| `http_shape` | string | summary method/path/query |
| `suite_a` | bool/ref | dedicated Suite A coverage when supported |
| `suite_b_tools` | string[] | matrix tools that require the op |
| `error_on_refuse` | string | XML `Code` when not supported / NYI |

**Validation**: Every Supported row has Suite A coverage before DoD. Status changes require spec/matrix update (FR-011).

**Relationships**: Consumed by classify (`compat`), docs generator, Suite A probes for Unsupported rows.

## 2. S3 Session

SigV4-authenticated session on one TCP connection (keep-alive MAY reuse).

| Field | Type | Notes |
|-------|------|-------|
| `principal_id` | PrincipalId | from `014` |
| `namespace` | NamespaceName | exactly one; unbound → auth fail |
| `access_key_id` | string | SigV4 credential |
| `security_token` | optional string | when temporary keys issued |
| `client_app` | string | from `User-Agent` |
| `options` | QueryOptions | quorum/timeout; headers or defaults |
| `profile` | DialectProfile | HandlersComplete / CompleteProduct |

**Transitions**: TCP accept → signature peek → auth verify → bound session → request loop → drain/close. Auth failure closes without data access.

**Validation**: FR-006–FR-009; clock skew; signature match before dispatch.

## 3. Bucket (S3 view)

| Field | Type | Notes |
|-------|------|-------|
| `name` | string | S3 bucket name; DNS-compatible per `002` regex; unique **per namespace** |
| `namespace` | NamespaceName | binding |
| `container` | ContainerRef | underlying datatype container |
| `type_name` | TypeName | default `object_collection` |
| `created_at` | timestamp | ListBuckets CreationDate |
| `location` | string | constant region (default `us-east-1`) |

**Validation**: Duplicate create → `BucketAlreadyOwnedByYou` / `BucketAlreadyExists`. Delete non-empty (objects or in-progress multipart) → `BucketNotEmpty`. Names escaping: containers that violate S3 naming use `002` `x--<hex>` exposure for SpaceStorage listing only.

**Relationships**: 1:N Object keys; 1:N Multipart uploads.

## 4. Object

| Field | Type | Notes |
|-------|------|-------|
| `bucket` | Bucket name | |
| `key` | string | binary-safe UTF-8 |
| `body` | bytes | streamed; not required resident in handler RAM |
| `size` | u64 | |
| `etag` | string | single-part content-derived; multipart composite |
| `content_type` | string | |
| `user_metadata` | map | `x-amz-meta-*` |
| `cache_control` / disposition / encoding | optional | supported subset |
| `last_modified` | timestamp | |
| `checksums` | optional | stored when client supplied supported algos |

**Validation**: `max_key` / `max_value` (`015`); empty body allowed; overwrite last-writer-wins atomic publish; interrupted single PUT → not readable.

**Relationships**: Belongs to one Bucket; optional Multipart origin.

## 5. Multipart Upload

| Field | Type | Notes |
|-------|------|-------|
| `upload_id` | string | opaque; namespace-scoped |
| `bucket` / `key` | string | |
| `namespace` | NamespaceName | |
| `initiated_at` | timestamp | |
| `parts` | Part[] | part_number, etag/md5, size, durable ref |
| `state` | enum | `in_progress` \| `completed` \| `aborted` |
| `idle_deadline` | timestamp | from `multipart_idle` |

**Part rules**: part_number 1..=10000; intermediate parts ≥ 5 MiB; last may be smaller; complete order validated → else `InvalidPartOrder` / `InvalidPart`; abort removes parts; cross-namespace upload id reuse → fail.

**Transitions**: Create → UploadPart* → Complete (publish Object) | Abort | idle cleanup.

## 6. Presigned Request

| Field | Type | Notes |
|-------|------|-------|
| `method` | HTTP method | |
| `credential` / `signed_headers` / `signature` | SigV4 query | |
| `expires` | duration | `X-Amz-Expires` |
| `namespace` | from credential | not forgeable via URL alone |

**Validation**: Expiry; signature; namespace/authz still applied (FR-008).

## 7. Suite A Run / Suite B Run

| Field | Type | Notes |
|-------|------|-------|
| `config_snapshot` | endpoint, creds, limits, region | |
| `results` | per-operation or per-tool pass/fail | |
| `sha256_pairs` | fixture hash ↔ read hash | mandatory on success paths |
| `exit_code` | int | Suite B non-zero on any fail |

## 8. Matrix Tool Row

| Field | Type | Notes |
|-------|------|-------|
| `tool_id` | enum | rclone, awscli, boto3, s3fs, clickhouse-backup, elasticsearch, pgbackrest, restic |
| `scenarios` | list | write/read/verify/… |
| `status` | pass/fail | |
| `failure_excerpt` | optional string | |

## State diagrams (summary)

```text
Session:  Accept → Auth → Bound ⟲ Request → Close
Object:   (absent) → Put/Complete → Present → Delete → absent
Multipart: Create → InProgress ⟲ Part → Complete|Abort|IdleAbort
```

## Mapping to existing crates

| Entity | Primary owner |
|--------|----------------|
| Matrix | this feature contracts + `compat::classify_s3` |
| Session / principal | `authz` + `handler-s3` |
| Bucket / Object | `types` object_collection + `query`/`storage` |
| Multipart | `handler-s3` registry + durable store |
| Suite runs | `conformance` + `tests/s3-interop` |
