# Conformance tests

## S3 Suite A (`dialect_s3_*`)

Integration targets (each file is its own test binary). Cargo's name filter also filters *functions*, so run the targets:

```bash
cargo test -p spacestorage-conformance --features handlers-complete \
  --test dialect_s3_auth_foundation \
  --test dialect_s3_isolation \
  --test dialect_s3_limits \
  --test dialect_s3_multipart \
  --test dialect_s3_ops \
  --test dialect_s3_perf \
  --test dialect_s3_query_options \
  --test dialect_s3_semantics \
  --test dialect_s3_sigv4 \
  --test dialect_s3_unsupported
```

`handlers-complete` is an empty feature on this crate: Suite A is in the default build, with **zero** mandatory `#[ignore]`. A filter of `dialect_s3_` matches test *function* names and runs nothing — use the `--test` list above. There is no `first-binary` Cargo feature. FirstBinary regression is:

```bash
cargo test -p spacestorage-conformance --test release_profile_s3 --test handlers_absent
```

`compat_handlers_complete.rs` and `compat_wire_handlers_complete.rs` stay smaller S3 smokes; they do not replace Suite A.

| Module | Coverage |
|--------|----------|
| `dialect_s3_ops` | Supported bucket/object ops, pagination, checksums, SHA256 read-back |
| `dialect_s3_sigv4` | Header, presign, UNSIGNED, streaming payload modes |
| `dialect_s3_semantics` | Metadata, Range, conditionals, keep-alive, 100-continue, chunked, interrupt |
| `dialect_s3_multipart` | Lifecycle, ETag, malformed complete, unknown upload, part order |
| `dialect_s3_isolation` | Unbound credentials, two namespaces, presign |
| `dialect_s3_limits` | `max_key` / `max_value`, multipart sum, connection cap, raised fixture |
| `dialect_s3_unsupported` | Unsupported-by-design and NYI probes |
| `dialect_s3_perf` | Streaming window proxy, N = 8 workers, multipart idle |

### SC-007 peak RSS

Objects larger than `CHUNK_WINDOW` (64 KiB) are stored and served as fixed windows. The handler must not assemble a second full-object buffer on that path. Process RSS still includes the in-memory catalog's chunk copies, so the suite asserts the **window proxy**: response bodies are HTTP-chunked and SHA256 matches, for two sizes both above the window. Concurrent workers: **N = 8**.