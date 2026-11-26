# Contract: S3 Capability Matrix

**Feature**: `019-s3-protocol` | Spec FR-011, FR-003–FR-005, FR-057 | Extends `015` S3 rows and `002` `protocols/s3.md`

## Status vocabulary

| Status | Meaning | Runtime | Tests |
|--------|---------|---------|-------|
| **Supported and tested** | MUST work | Success per [operations.md](operations.md) | Suite A dedicated coverage; Suite B where tool uses it |
| **Unsupported by design** | Product non-goal | Explicit not-supported / `NotImplemented` / `501` | Suite A probes (never silent success) |
| **Not yet implemented** | Placeholder | Predictable `NotImplemented` naming the op | Suite A probe; promote via spec change |

## Supported and tested

| Operation | HTTP summary | Notes |
|-----------|--------------|-------|
| ListBuckets | `GET /` | Namespace-scoped |
| HeadBucket | `HEAD /{bucket}` | |
| CreateBucket | `PUT /{bucket}` | LocationConstraint ignored |
| DeleteBucket | `DELETE /{bucket}` | Empty only |
| GetBucketLocation | `GET /{bucket}?location` | Constant region |
| PutObject | `PUT /{bucket}/{key}` | Metadata, checksums, conditionals subset |
| GetObject | `GET /{bucket}/{key}` | Range, conditionals, response-* subset |
| HeadObject | `HEAD /{bucket}/{key}` | |
| DeleteObject | `DELETE /{bucket}/{key}` | |
| DeleteObjects | `POST /{bucket}?delete` | Quiet mode |
| CopyObject | `PUT` + `x-amz-copy-source` | Same-namespace |
| ListObjectsV2 | `GET ?list-type=2` | prefix, delimiter, continuation-token, start-after, max-keys, encoding-type=url |
| ListObjects | `GET /{bucket}` (v1) | marker, prefix, delimiter, max-keys |
| CreateMultipartUpload | `POST ?uploads` | |
| UploadPart | `PUT ?uploadId&partNumber` | |
| CompleteMultipartUpload | `POST ?uploadId` | |
| AbortMultipartUpload | `DELETE ?uploadId` | |
| ListParts | `GET ?uploadId` | |
| ListMultipartUploads | `GET /{bucket}?uploads` | |
| Presigned URL | query SigV4 | GET/PUT methods used by boto3 |

Auth modes (Supported and tested): header SigV4; presigned; `UNSIGNED-PAYLOAD`; `STREAMING-AWS4-HMAC-SHA256-PAYLOAD`; `STREAMING-UNSIGNED-PAYLOAD-TRAILER`.

## Unsupported by design

| Capability | Error expectation |
|------------|-------------------|
| Bucket versioning enable/suspend | `501` / `NotImplemented` |
| Object lock / legal hold | not supported |
| S3 replication configuration | not supported |
| SelectObjectContent | not supported |
| ACL / canned ACL beyond `014` mapping | reject or documented ignore |
| Bucket lifecycle | not supported |
| CORS / website / policy XML writes | not supported |
| SSE as AWS KMS (`x-amz-server-side-encryption`) | `501` until product mapping (keyed on the SSE header only, never on checksum headers) |
| Object tagging API | `501 NotImplemented` |
| SigV2 | `InvalidArgument` |
| S3 Control / Batch Jobs | out of data-plane scope |

## Not yet implemented

| Capability | Notes |
|------------|-------|
| UploadPartCopy | `NotImplemented` until promoted |
| GetObjectAttributes | `NotImplemented` until promoted |

## Machine-readable twin

Implement MUST emit `s3-compatibility-matrix.json` with the same rows plus `suite_a_pass`, `suite_b_tools[]`, and git/release tag for SC-008 audits.

## Relationship to `015`

`015` MUST/MUST NOT ceiling remains. This matrix **adds** required ops (HeadBucket, DeleteObjects, CopyObject, ListObjects v1, GetBucketLocation, full multipart lifecycle, presign) for complete-product S3 dialect ownership under FR-011.
