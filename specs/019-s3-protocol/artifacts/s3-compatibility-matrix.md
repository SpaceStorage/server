# S3 compatibility matrix

Generated from `specs/019-s3-protocol/contracts/capability-matrix.md`.

Suite A marks are true for Supported rows when this file is regenerated after the dialect_s3 test binaries pass. Suite B is true only when `tests/s3-interop/matrix/results.json` has no failed row.

| Operation | Status | Suite A | Suite B |
|---|---|---|---|
| ListBuckets | Supported and tested | true | true |
| HeadBucket | Supported and tested | true | true |
| CreateBucket | Supported and tested | true | true |
| DeleteBucket | Supported and tested | true | true |
| GetBucketLocation | Supported and tested | true | true |
| PutObject | Supported and tested | true | true |
| GetObject | Supported and tested | true | true |
| HeadObject | Supported and tested | true | true |
| DeleteObject | Supported and tested | true | true |
| DeleteObjects | Supported and tested | true | true |
| CopyObject | Supported and tested | true | true |
| ListObjectsV2 | Supported and tested | true | true |
| ListObjects | Supported and tested | true | true |
| CreateMultipartUpload | Supported and tested | true | true |
| UploadPart | Supported and tested | true | true |
| CompleteMultipartUpload | Supported and tested | true | true |
| AbortMultipartUpload | Supported and tested | true | true |
| ListParts | Supported and tested | true | true |
| ListMultipartUploads | Supported and tested | true | true |
| Presigned URL | Supported and tested | true | true |
| Bucket versioning enable/suspend | Unsupported by design | false | false |
| Object lock / legal hold | Unsupported by design | false | false |
| S3 replication configuration | Unsupported by design | false | false |
| SelectObjectContent | Unsupported by design | false | false |
| ACL / canned ACL beyond `014` mapping | Unsupported by design | false | false |
| Bucket lifecycle | Unsupported by design | false | false |
| CORS / website / policy XML writes | Unsupported by design | false | false |
| SSE as AWS KMS (`x-amz-server-side-encryption`) | Unsupported by design (SSE header only; checksum headers never trigger) | false | false |
| Object tagging API | Unsupported by design | false | false |
| SigV2 | Unsupported by design | false | false |
| S3 Control / Batch Jobs | Unsupported by design | false | false |
| UploadPartCopy | Not yet implemented | false | false |
| GetObjectAttributes | Not yet implemented | false | false |
