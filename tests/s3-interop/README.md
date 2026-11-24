# S3 client interoperability (Suite B)

Stock S3 clients in **Docker** against an **already running** SpaceStorage S3 endpoint.
Client images do **not** build `spacestoraged` (FR-051).

| Row | Client |
|-----|--------|
| rclone | rclone |
| awscli | AWS CLI (`aws s3` / `aws s3api`) |
| boto3 | Python boto3 |
| s3fs | s3fs-fuse (needs `/dev/fuse`) |
| clickhouse-backup | Altinity clickhouse-backup |
| elasticsearch | Elasticsearch S3 snapshot repository |
| pgbackrest | pgBackRest (repo-type S3) |
| restic | Restic |
| aws-sdk-go-v2 | Second SDK smoke (research R13) |

Each row writes, reads back, and verifies **SHA256**. The harness exits non-zero if any row fails.

---

## Prerequisites

- Docker Desktop / Engine (Linux containers). On macOS/Windows, `host.docker.internal` must reach the host (Compose adds `host-gateway`).
- Workspace root is the Spec Kit / Cargo root: `server/`.
- For large / backup tools: raised object size limit (`max_value` ≥ ~1 GiB), as in `specs/019-s3-protocol/contracts/fixtures/s3-limits-raised.conf`.
- Namespace-bound credentials (the in-process demo listener uses `demo` / `demo`).

---

## Manual mode (recommended one-shot)

From the **workspace root** (`server/`):

```bash
# Builds/runs the in-process S3 example on :19001 (demo/demo), then every Docker row.
./tests/s3-interop/harness/start-local.sh
```

What it does:

1. Starts `cargo run -p spacestorage-handler-s3 --example s3_listen` on `0.0.0.0:19001` if nothing is listening yet (log: `target/s3_listen.log`, pid: `target/s3_listen.pid`).
2. Sets `S3_ENDPOINT=http://127.0.0.1:19001` and maps Docker → `http://host.docker.internal:19001`.
3. Runs `./harness/run.sh` (build + `docker compose run` per row).
4. Writes `tests/s3-interop/matrix/results.json` and `matrix/<row>.log`.

Expect: every row `PASS`, harness exit `0`, SHA256 lines in the logs.

Stop the listener when finished:

```bash
kill "$(cat target/s3_listen.pid)" 2>/dev/null || true
```

---

## Manual mode (split: server already up)

Use this when you already have `spacestoraged` or `s3_listen` listening.

### A. In-process demo listener

```bash
# Terminal 1 — workspace root
export S3_LISTEN=0.0.0.0:19001
export S3_VHOST_SUFFIX=s3.local
export S3_MAX_VALUE=1GiB S3_MAX_KEY=4KiB
cargo run -p spacestorage-handler-s3 --example s3_listen
```

### B. Full daemon (complete-product / handlers-complete profile)

```bash
# Use a conf with a dedicated S3 entrypoint and raised limits, e.g.:
#   specs/019-s3-protocol/contracts/fixtures/s3-limits-raised.conf
# Mint a namespace-bound access key via admin / 014 surfaces, then:
export S3_ENDPOINT=http://127.0.0.1:<s3-port>
export S3_ACCESS_KEY_ID=<key>
export S3_SECRET_ACCESS_KEY=<secret>
```

### C. Run the Docker matrix

```bash
cd tests/s3-interop
export S3_ENDPOINT=http://127.0.0.1:19001   # or your daemon port
export S3_ACCESS_KEY_ID=demo
export S3_SECRET_ACCESS_KEY=demo
export S3_REGION=us-east-1
export S3_BUCKET=suiteb
export S3_VHOST_SUFFIX=s3.local             # optional; enables vhost rows

# run.sh derives S3_ENDPOINT_DOCKER from 127.0.0.1/localhost endpoints
./harness/run.sh
```

Single client (debug):

```bash
cd tests/s3-interop
export S3_ENDPOINT=http://127.0.0.1:19001
export S3_ENDPOINT_DOCKER=http://host.docker.internal:19001
export S3_ACCESS_KEY_ID=demo S3_SECRET_ACCESS_KEY=demo
export S3_REGION=us-east-1 S3_BUCKET=suiteb S3_VHOST_SUFFIX=s3.local
export AWS_ACCESS_KEY_ID=demo AWS_SECRET_ACCESS_KEY=demo AWS_DEFAULT_REGION=us-east-1
export AWS_EC2_METADATA_DISABLED=true

docker compose run --rm --build rclone    # or awscli, boto3, …
# logs for a full harness run: matrix/<row>.log
```

---

## Manual smoke without Docker (host AWS CLI)

```bash
export AWS_ACCESS_KEY_ID=demo AWS_SECRET_ACCESS_KEY=demo AWS_DEFAULT_REGION=us-east-1
aws --endpoint-url http://127.0.0.1:19001 s3 mb s3://suiteb 2>/dev/null || true
echo hello | aws --endpoint-url http://127.0.0.1:19001 s3 cp - s3://suiteb/manual.txt
aws --endpoint-url http://127.0.0.1:19001 s3 cp s3://suiteb/manual.txt - | shasum -a 256
```

---

## Interpreting results

| Artifact | Meaning |
|----------|---------|
| `matrix/results.json` | Aggregate `pass` / `exit` / short `detail` per row |
| `matrix/<row>.log` | Full compose / client stdout+stderr |
| Exit code of `run.sh` / `start-local.sh` | `0` = all rows passed; non-zero = at least one failure |

After a green run, refresh the published matrix:

```bash
# from workspace root
rustc scripts/s3-matrix-gen.rs -o target/s3-matrix-gen
./target/s3-matrix-gen
# writes docs/s3-compatibility-matrix.{md,json} and specs/019-s3-protocol/artifacts/
```

---

## Troubleshooting

| Symptom | What to check |
|---------|----------------|
| `S3_ENDPOINT unset` | Export `S3_ENDPOINT` or use `start-local.sh` |
| `S3_ENDPOINT_DOCKER is missing` | Prefer `./harness/run.sh` (sets it); or export `S3_ENDPOINT_DOCKER=http://host.docker.internal:<port>` |
| Connection refused from container | Listener bound to `0.0.0.0`, not only `127.0.0.1`; `extra_hosts` includes `host-gateway` |
| s3fs fails | Privileged + `/dev/fuse` (Compose already sets this); Docker Desktop file sharing / OS fuse support |
| pgBackRest TLS errors | Client row terminates TLS in-container and forwards plaintext to the HTTP listener — see `clients/pgbackrest/` |
| Elasticsearch slow / OOM | Row needs ~512 MiB heap; first pull/build is heavy |
| Idle-channel `NoSuchKey` in rclone/clickhouse-backup logs | Harmless probe noise if the row still prints `sha256 ok` and exits 0 |

---

## Suite A (protocol conformance, no Docker)

```bash
# workspace root
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

Do **not** use a bare `dialect_s3_` name filter — it matches function names and can run nothing. See also `crates/conformance/tests/README.md` and `specs/019-s3-protocol/quickstart.md`.

---

## CI shape

1. Start S3 endpoint (daemon with raised limits, or `s3_listen`).
2. Suite A via the command above.
3. `cd tests/s3-interop && ./harness/run.sh` (or `./harness/start-local.sh` for the demo listener).
4. Regenerate `docs/s3-compatibility-matrix.{md,json}` with `scripts/s3-matrix-gen.rs`.
