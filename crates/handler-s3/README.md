# spacestorage-handler-s3

Complete-product S3 REST dialect (SigV4, path-style + optional virtual-hosted).

## Module map

| Module | Role |
|--------|------|
| `handler.rs` | HTTP/1.1 wire, auth gate, keep-alive |
| `router.rs` | Method/path/query → classified verb |
| `dispatch.rs` | Classify-gated ops; `spawn_blocking` for durable/catalog path |
| `sigv4.rs` | AWS SigV4 verification |
| `xml.rs` | S3 XML responses |
| `multipart.rs` | Upload registry |
| `stream.rs` | Streaming I/O (no full-object buffer on large path) |
| `checksum.rs` / `etag.rs` | Integrity (US4) |

## Tests

```bash
cargo test -p spacestorage-handler-s3
```

Suite A lives in `crates/conformance/tests/dialect_s3_*` (live TCP).