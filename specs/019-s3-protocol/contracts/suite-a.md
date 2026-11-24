# Contract: Suite A — S3 Protocol Conformance

**Feature**: `019-s3-protocol` | Spec User Story 6, FR-050, FR-054–FR-057, DoD

## Purpose

Independent live-server tests for every **Supported and tested** operation and for semantics / SigV4 / isolation / limits / Unsupported probes. Mandatory cases MUST NOT rely exclusively on mocks.

## Location

`crates/conformance/tests/dialect_s3_*.rs` (names indicative):

| Module | Coverage |
|--------|----------|
| `dialect_s3_ops` | Each Supported op: positive path + SHA256 round-trip |
| `dialect_s3_sigv4` | Header, presign, UNSIGNED, streaming modes, skew, bad sig, session token |
| `dialect_s3_semantics` | Metadata, Range, conditionals, overwrite, 100-continue, chunked, keep-alive, interrupt |
| `dialect_s3_multipart` | Full lifecycle, ListParts, Abort, ListMultipartUploads, ETag composite |
| `dialect_s3_isolation` | Unbound refuse; two namespaces; cross presign; cross upload id |
| `dialect_s3_limits` | Named errors for size/connection; raised-limits fixture |
| `dialect_s3_unsupported` | Unsupported by design + NYI → explicit errors |
| `dialect_s3_perf` | Large-object memory ceiling smoke; concurrent N; multipart idle |

## Gate

```bash
cargo test -p spacestorage-conformance --features handlers-complete dialect_s3_
```

Complete-product CI also runs the same modules under the complete-product feature set when that profile is the merge gate.

## Requirements

1. Zero mandatory skips for Supported ops.
2. Every successful write records SHA256 at put and verifies on get.
3. Negatives assert HTTP status and XML `Code` ([errors.md](errors.md)).
4. Pagination boundary: exactly `max-keys` objects → continuation has no dup/miss.
5. Peak memory assertion documented (ceiling constant in test README).
6. Existing `compat_handlers_complete` / `compat_wire_handlers_complete` S3 smokes MAY remain as subset; Suite A supersedes them for dialect DoD.

## Out of Suite A

Docker client images (Suite B). FirstBinary (no S3 handler).
