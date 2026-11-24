# Quickstart: S3 Protocol (019)

Validation guide for the complete-product S3 dialect. Implementation details belong in `tasks.md` (later). Contracts: [capability-matrix.md](contracts/capability-matrix.md), [operations.md](contracts/operations.md), [sigv4.md](contracts/sigv4.md), [suite-a.md](contracts/suite-a.md), [suite-b.md](contracts/suite-b.md).

## Prerequisites

- Workspace root: `server/`
- Build profile that **includes** the `s3` handler (`handlers-complete` or `complete-product`). FirstBinary builds intentionally omit S3.
- Namespace-bound principal with SigV4 access key + secret and WRITE (mint via `014` / admin surfaces).
- For Suite B: Docker available; raised `limits.max_value` conf for backup tools.

## 1. Start a node with an S3 entrypoint

Fixtures (exact files):

- `specs/019-s3-protocol/contracts/fixtures/s3-entrypoint.conf`
- `specs/019-s3-protocol/contracts/fixtures/s3-limits-raised.conf` (Suite B / large objects, `max_value 1GiB`)
- `specs/019-s3-protocol/contracts/fixtures/s3-vhost.conf`

Use a conf that declares a dedicated S3 port (example shape — exact directives in [config-directives.md](contracts/config-directives.md)):

```text
entrypoint s3 { address 127.0.0.1:19000; plaintext; }
protocols.s3 { region us-east-1; /* optional: virtual_host_suffix s3.local; */ }
limits { max_value 1GiB; }   # for large / Suite B; keep defaults for small smoke
```

Boot `spacestoraged` with that conf. Confirm no other handler shares `19000`.

## 2. Smoke with AWS CLI (path-style)

```bash
export AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... AWS_DEFAULT_REGION=us-east-1
aws --endpoint-url http://127.0.0.1:19000 s3 mb s3://demo
echo hello | aws --endpoint-url http://127.0.0.1:19000 s3 cp - s3://demo/hello.txt
aws --endpoint-url http://127.0.0.1:19000 s3 cp s3://demo/hello.txt - | shasum -a 256
```

Expect: bucket create, put/get round-trip, SHA256 of `hello\n` (or fixture) matches.

## 3. Suite A (protocol conformance)

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

A bare `dialect_s3_` filter also filters function names and runs nothing. Expect: all mandatory modules pass; zero mandatory skips; negatives show correct XML `Code`. Local run is well under 60 minutes.

## 4. Suite B (eight clients + second SDK)

Full manual steps, single-client debug, troubleshooting: **[`tests/s3-interop/README.md`](../../tests/s3-interop/README.md)**.

```bash
# One-shot: demo listener on :19001 + all Docker rows (SHA256 verify)
./tests/s3-interop/harness/start-local.sh

# Or, endpoint already up:
# cd tests/s3-interop
# export S3_ENDPOINT=http://127.0.0.1:19001 S3_ACCESS_KEY_ID=demo S3_SECRET_ACCESS_KEY=demo
# export S3_REGION=us-east-1 S3_BUCKET=suiteb S3_VHOST_SUFFIX=s3.local
# ./harness/run.sh
```

Expect: nine rows (eight tools + aws-sdk-go-v2) pass; harness exit 0; each successful write SHA256-verified. Connection failure → non-zero, not pass. Results: `tests/s3-interop/matrix/results.json`.

## 5. boto3 + second SDK

From Suite B boto3 service (or documented sibling): path-style put/get, optional virtual-hosted, presigned GET/PUT, multipart upload_fileobj, Range get, unsupported API → `ClientError` with documented `Code`. Second SDK smoke (Go v2 or JS) runs in the same gate.

## 6. Isolation check

Two namespaces, same bucket display name, two credential pairs: neither lists the other’s buckets/keys; cross-namespace presign and multipart upload id reuse fail.

## 7. Unsupported probes

Enable versioning / Select / object lock APIs → `NotImplemented` / `501`, never empty 200.

## 8. Matrix artifact

After A+B green, generate `s3-compatibility-matrix.{md,json}` and confirm every Supported row is tested and statuses match [capability-matrix.md](contracts/capability-matrix.md) (SC-008).

## First-binary regression

`spacestorage-conformance` has no `first-binary` Cargo feature. The daemon is one build; FirstBinary is the release profile asserted inside the tests. S3 stays forbidden, and an `entrypoint s3` conf fails `entrypoint_unknown_handler`. Suite A (`dialect_s3_*`) is not part of this regression.

```bash
cargo test -p spacestorage-conformance --test release_profile_s3 --test handlers_absent
```

Do **not** claim FirstBinary S3 support.
