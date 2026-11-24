# Contract: S3 XML Errors

**Feature**: `019-s3-protocol` | Spec FR-027, FR-004–FR-005, FR-012

## Body shape

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>...</Code>
  <Message>...</Message>
  <Resource>...</Resource>
  <RequestId>...</RequestId>
</Error>
```

`Content-Type: application/xml`. `RequestId` matches `x-amz-request-id`.

## Stable codes (minimum)

| Code | Typical HTTP | When |
|------|--------------|------|
| `NoSuchBucket` | 404 | Missing bucket |
| `NoSuchKey` | 404 | Missing object / copy source |
| `BucketAlreadyOwnedByYou` | 409 | Duplicate create same owner |
| `BucketAlreadyExists` | 409 | Name conflict rules |
| `BucketNotEmpty` | 409 | DeleteBucket |
| `InvalidArgument` | 400 | Bad params; SigV2 |
| `InvalidBucketName` | 400 | Name regex |
| `MalformedXML` | 400 | Bad XML bodies |
| `EntityTooLarge` | 400 | Over `max_value` / limits |
| `EntityTooSmall` | 400 | Multipart part too small |
| `InvalidPart` | 400 | Bad part on complete |
| `InvalidPartOrder` | 400 | Part order |
| `NoSuchUpload` | 404 | Unknown upload id |
| `PreconditionFailed` | 412 | Conditionals |
| `InvalidRange` | 416 | Bad Range |
| `SignatureDoesNotMatch` | 403 | Bad sig |
| `InvalidAccessKeyId` | 403 | Unknown key |
| `AccessDenied` | 403 | Authz / anonymous / expired presign |
| `RequestTimeTooSkewed` | 403 | >15 min skew |
| `NotImplemented` | 501 | Unsupported by design / NYI |
| `BadDigest` | 400 | Content-MD5 mismatch |
| `InvalidRequest` | 400 | Type mismatch / invalid state |
| `RequestTimeout` | 504 | Timeout |
| `QuorumUnsatisfiable` | 503 | Extension; quorum not met |
| `limit_exceeded` | 400/503 | Named `015` limit (message carries limit/current/max) |

`304 Not Modified` has empty/no Error body per HTTP semantics.

## Rules

- Unsupported / NYI: **never** HTTP 200 with empty success.
- Extension codes remain listed in the published matrix.
- Suite A negative cases assert HTTP status **and** `Code`.
