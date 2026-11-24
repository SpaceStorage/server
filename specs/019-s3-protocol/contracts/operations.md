# Contract: S3 Supported Operations

**Feature**: `019-s3-protocol` | Spec User Story 1, FR-003, FR-020–FR-027 | Aligns with `002` `protocols/s3.md`

## Addressing

| Mode | Form | When |
|------|------|------|
| Path-style | `/{bucket}` or `/{bucket}/{key}` | Always |
| Virtual-hosted | `Host: {bucket}.{virtual_host_suffix}` + path `/{key}` | Suffix configured |

## Common response headers

- `x-amz-request-id`, `x-amz-id-2` on every response
- `ETag` on Put/Complete/Head/Get as applicable
- Optional `X-SpaceStorage-Applied-*`, `X-SpaceStorage-Options-Source`

## Bucket operations

### ListBuckets — `GET /`

XML `ListAllMyBucketsResult` with buckets in the bound namespace only (no namespace prefix on names). Optional extension element `SpaceStorageType` ignored by stock clients.

### HeadBucket — `HEAD /{bucket}`

`200` if exists and authorized; `404` `NoSuchBucket` otherwise.

### CreateBucket — `PUT /{bucket}`

Accepts optional `CreateBucketConfiguration` XML (`LocationConstraint` ignored). Default type `object_collection`. Headers `x-spacestorage-type`, `x-spacestorage-opt-*` honored. Duplicate → `BucketAlreadyOwnedByYou` / `BucketAlreadyExists`.

### DeleteBucket — `DELETE /{bucket}`

Requires empty of objects and in-progress multipart uploads → else `BucketNotEmpty`.

### GetBucketLocation — `GET /{bucket}?location`

Returns configured region constant (default `us-east-1`).

## Object operations

### PutObject — `PUT /{bucket}/{key}`

Body streamed. Supports `Content-Type`, `x-amz-meta-*`, `Cache-Control`, `Content-Disposition`/`Encoding`, `Content-MD5`, `x-amz-checksum-sha256|crc32|crc32c`, `If-None-Match: *`. Success → `200` + ETag. Zero-length body allowed.

### GetObject — `GET /{bucket}/{key}`

Full or `Range` (`206` / `416`). Conditionals: `If-Match` / `If-None-Match` / `If-Modified-Since` / `If-Unmodified-Since` → `304` / `412` per AWS rules for supported ops. `response-content-*` override subset per `002`.

### HeadObject — `HEAD /{bucket}/{key}`

Metadata without body.

### DeleteObject — `DELETE /{bucket}/{key}`

Idempotent success shape for missing key per AWS-compatible behavior documented in Suite A.

### DeleteObjects — `POST /{bucket}?delete`

XML delete body; `quiet` mode supported.

### CopyObject — `PUT /{bucket}/{key}` with `x-amz-copy-source`

Same-namespace copy; `x-amz-metadata-directive` `COPY`|`REPLACE`. Missing source → `NoSuchKey`.

### ListObjectsV2 — `GET /{bucket}?list-type=2`

Parameters: `prefix`, `delimiter`, `continuation-token`, `start-after`, `max-keys`, `encoding-type=url`. Pagination: no duplicate/missing keys vs ground truth.

### ListObjects (v1) — `GET /{bucket}`

Parameters: `marker`, `prefix`, `delimiter`, `max-keys`. Key sets consistent with V2 for overlapping filters.

## Multipart

| Op | Shape |
|----|-------|
| CreateMultipartUpload | `POST /{bucket}/{key}?uploads` |
| UploadPart | `PUT /{bucket}/{key}?uploadId&partNumber` |
| CompleteMultipartUpload | `POST /{bucket}/{key}?uploadId` + XML parts |
| AbortMultipartUpload | `DELETE /{bucket}/{key}?uploadId` |
| ListParts | `GET /{bucket}/{key}?uploadId` |
| ListMultipartUploads | `GET /{bucket}?uploads` |

Limits: max 10 000 parts; min intermediate part size 5 MiB; assembled size ≤ effective `max_value`. ETag on complete: `"<md5-of-part-md5s>-<N>"`.

## Quorum / timeout options

Headers `X-SpaceStorage-Quorum`, `X-SpaceStorage-Timeout`, `X-SpaceStorage-Session` (MAY be omitted from `SignedHeaders`). Defaults from `query_defaults` / admin config when unset.

## Classify verb names

Handlers map HTTP to uppercase verbs for `compat::classify` (non-exhaustive): `LISTBUCKETS`, `HEADBUCKET`, `CREATEBUCKET`, `DELETEBUCKET`, `GETBUCKETLOCATION`, `PUTOBJECT`, `GETOBJECT`, `HEADOBJECT`, `DELETEOBJECT`, `DELETEOBJECTS`, `COPYOBJECT`, `LISTOBJECTSV2`, `LISTOBJECTS`, `CREATEMULTIPARTUPLOAD`, `UPLOADPART`, `COMPLETEMULTIPARTUPLOAD`, `ABORTMULTIPARTUPLOAD`, `LISTPARTS`, `LISTMULTIPARTUPLOADS`, plus MustNot verbs `VERSIONING`, `OBJECTLOCK`, `REPLICATION`, `SELECT`, `ACL`, `UPLOADPARTCOPY`, `GETOBJECTATTRIBUTES`, …
