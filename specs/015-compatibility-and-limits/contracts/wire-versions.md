# Contract: Wire versions

**Feature**: `015-compatibility-and-limits` | Spec: FR-001, FR-002

| Handler | Wire |
|---------|------|
| `postgresql` | Frontend/backend **3.0**. SSLRequest honoured when entrypoint is `tls`; otherwise `N`. GSSENCRequest → `N`. CancelRequest cancels in-flight (`002`). |
| `cassandra` | Native **v4 and v5** (OPTIONS/STARTUP/QUERY/EXECUTE/PREPARE/BATCH/REGISTER as applicable). |
| `redis` | **RESP2**. RESP3 not in current profiles. |
| `elasticsearch` | HTTP/1.1 REST subset in [elasticsearch-search.md](elasticsearch-search.md). |
| `clickhouse` | Native Hello / query / data. |
| `clickhouse-http` | HTTP query / insert. Own entrypoint. Both CH handlers required in HandlersComplete+. |
| `s3` | SigV4 REST; ListBuckets, CreateBucket, Put/Get/DeleteObject, multipart, ListObjectsV2. |
| `webdav` | RFC 4918 subset: PROPFIND, GET, PUT, DELETE, MKCOL, MOVE, COPY. |

Mismatch: first-bytes signature, 1 s deadline, no session allocation (`002` FR-004).
