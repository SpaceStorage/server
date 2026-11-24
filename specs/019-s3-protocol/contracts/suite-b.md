# Contract: Suite B — Client Interoperability Harness

**Feature**: `019-s3-protocol` | Spec User Stories 7–8, FR-051–FR-053, FR-056, FR-060–FR-061

## Purpose

Docker Compose (or equivalent) runs **eight** stock clients against a **live external** SpaceStorage S3 endpoint. Client images MUST NOT build `spacestoraged`.

## Layout

```text
tests/s3-interop/
├── docker-compose.yml
├── README.md                 # single-command local/CI run
├── harness/                  # orchestrator: config, run rows, aggregate, exit code
├── clients/
│   ├── rclone/
│   ├── awscli/               # aws s3 + aws s3api
│   ├── boto3/                # dedicated FR-053 scenarios
│   ├── s3fs/
│   ├── clickhouse-backup/    # Altinity path
│   ├── elasticsearch/        # S3 snapshot repository (+ companion ES if needed)
│   ├── pgbackrest/           # repo-type S3 (+ companion if needed)
│   └── restic/
└── matrix/                   # machine-readable row results for matrix generator
```

## Configuration (harness env / file)

| Input | Required |
|-------|----------|
| Endpoint URL | yes |
| Region / custom endpoint | yes |
| Access key + secret (+ session token if used) | yes — namespace-bound WRITE |
| Bucket (or auto-create) | yes |
| TLS trust / plaintext | yes |
| Virtual-host suffix (optional) | for vhost boto3 row |

## Per-tool minimum scenarios

Each row MUST cover that tool’s relevant ops among: write, read, **SHA256 verify**, overwrite, delete, list, multipart where the tool uses it, interrupt recovery where supported.

| Tool | Notes |
|------|-------|
| rclone | copy/sync + list + checksum |
| AWS CLI | put/head/get/rm/cp + multipart where applicable |
| boto3 | path-style, virtual (if suffix), endpoint_url, region, presign, multipart, metadata, range, unsupported `ClientError.Code` |
| s3fs | mount, write, independent S3 get verify |
| clickhouse-backup | minimal artifact upload + list/get |
| Elasticsearch | register repo + minimal snapshot |
| pgBackRest | backup/check stores and verifies object |
| Restic | init, backup, check, restore small fixture |

## Second AWS SDK

In addition to boto3 and AWS CLI: ship a smoke for **aws-sdk-go-v2** *or* `@aws-sdk/client-s3` in the same CI gate (default Go v2 binary in harness). Document choice in `tests/s3-interop/README.md`.

## Pass/fail

- Structured per-row result (JSON/JUnit).
- Harness exit **non-zero** if any row fails or cannot reach endpoint (connection error ≠ pass).
- Missing companion dependency → that row fails with explicit message.

## CI shape

1. Start SpaceStorage (complete-product / handlers-complete build) with raised limits + namespace-bound keys.
2. Run Suite A (cargo).
3. Run Suite B (`docker compose …` or harness entrypoint).
4. Generate compatibility matrix artifact from results.
