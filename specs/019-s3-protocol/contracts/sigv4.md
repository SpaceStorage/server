# Contract: AWS Signature Version 4 (S3)

**Feature**: `019-s3-protocol` | Spec User Story 2, FR-002, Clarifications | `002` s3 Authentication row

## Algorithms

- Only `AWS4-HMAC-SHA256`. SigV2 → `400 InvalidArgument` ("SigV2 not supported").
- Service fixed to `s3` in credential scope. Region: any accepted; location API returns configured constant (default `us-east-1`).

## Header authentication

```
Authorization: AWS4-HMAC-SHA256 Credential=<access_key>/<date>/<region>/s3/aws4_request,
  SignedHeaders=..., Signature=...
```

Required companions: `x-amz-date` (or `Date`), `x-amz-content-sha256`.

Canonical request MUST follow AWS rules for URI and query encoding (Unicode, spaces, `#?&` in keys).

## Presigned (query) authentication

Query parameters: `X-Amz-Algorithm`, `X-Amz-Credential`, `X-Amz-Date`, `X-Amz-Expires`, `X-Amz-SignedHeaders`, `X-Amz-Signature`, optional `X-Amz-Security-Token`.

- Within expiry → authorize then execute.
- After expiry → `AccessDenied` or signature error (Suite A pins exact Code).
- Presign does **not** bypass namespace or authz (FR-008).

## Payload hash modes

| `x-amz-content-sha256` | Behavior |
|------------------------|----------|
| 64-hex lowercase SHA256 | Must match body (or streaming chunks as applicable) |
| `UNSIGNED-PAYLOAD` | Signature omits body hash; upload allowed |
| `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` | aws-chunked; per-chunk signatures verified |
| `STREAMING-UNSIGNED-PAYLOAD-TRAILER` | streaming unsigned with trailer support used by matrix clients |

Out of scope (trailer checksum modes not required by the eight tools). `x-amz-checksum-crc64nvme` is **accepted but not verified** — it is not a routing key and MUST NOT cause a refusal, since modern AWS SDK/CLI clients send it by default.

## Clock skew

Reject with `RequestTimeTooSkewed` when `|now - request_time| > 15 minutes`.

## Session tokens

When principal material includes a session token, `X-Amz-Security-Token` (header or query) MUST match. Missing/wrong/cross-namespace → authentication failure before data access.

## Credential binding

- `access_key` → `014` principal; secret from store.
- Principal MUST have exactly one namespace binding for S3; unbound → refuse.
- Failed auth: `SignatureDoesNotMatch` | `InvalidAccessKeyId` | `AccessDenied` | `RequestTimeTooSkewed` as appropriate — never return another tenant's data.
- Anonymous (no auth) → `403 AccessDenied`.

## Multipart

Each `UploadPart` is independently signed with the correct payload mode for that request.

## First-bytes / mismatch

Wrong protocol on the S3 port: refuse within 1 s per `002` / `015` handshake rules (`protocol-core` peek).
