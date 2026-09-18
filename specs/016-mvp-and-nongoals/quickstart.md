# Quickstart: first shippable binary (slices 1–5)

**Feature**: `016-mvp-and-nongoals` | Proves SC-001 (operator path < 60 minutes), SC-002, SC-003, SC-006

Contracts: [first-binary](contracts/first-binary.md), [dialects](contracts/dialect-first-binary.md), [conformance](contracts/conformance-profile.md), [fixtures](contracts/fixtures/).

This walkthrough is the human gate. Automated equivalent: `cargo test -p spacestorage-conformance --features first-binary`. Implementation of slices 1–5 must exist before the commands succeed; this file is the acceptance script, not the server source.

## Prerequisites

- Rust 1.87 (`rust-toolchain.toml`).
- Linux or macOS. Loopback ports `5432`–`5434`, `6379`–`6381`, `7700`–`7703`, `7800`–`7803`, `7900`–`7903` free.
- `psql` and `redis-cli` (stock clients). `jq` optional.
- Clock: the happy path below is sized to finish in under 60 minutes including reading errors.

## 0. Build the first binary (not the seven-protocol matrix)

```bash
cargo build --release -p spacestoraged -p spacestorage
# default features = postgresql + redis handlers only
ls target/release/spacestoraged target/release/spacestorage
```

Do not pass `--features complete-product` for this milestone.

## 1. Keys and token files

```bash
sudo mkdir -p /etc/spacestorage /var/lib/spacestorage/db-1
head -c 32 /dev/urandom | base64 | sudo tee /etc/spacestorage/admin.token >/dev/null
head -c 32 /dev/urandom | sudo tee /etc/spacestorage/master.key >/dev/null
head -c 32 /dev/urandom | sudo tee /etc/spacestorage/join.secret >/dev/null
sudo chmod 600 /etc/spacestorage/admin.token /etc/spacestorage/master.key /etc/spacestorage/join.secret
```

## 2. Reject bad configs (SC-002, FR-007)

```bash
spacestorage validate specs/016-mvp-and-nongoals/contracts/fixtures/invalid/unknown-handler-cassandra.conf
# expected: exit 2, code entrypoint_unknown_handler, handler=cassandra

spacestorage validate specs/016-mvp-and-nongoals/contracts/fixtures/invalid/omitted-transport.conf
# expected: exit 2, code transport_undeclared on admin-http
```

## 3. One-node start (Story 1)

```bash
sudo cp specs/016-mvp-and-nongoals/contracts/fixtures/first-binary-one-node.conf /etc/spacestorage/node.conf
spacestorage validate /etc/spacestorage/node.conf          # OK
spacestoraged --config /etc/spacestorage/node.conf
```

Expected within 10 s: `state=ready`; listeners on `127.0.0.1:{7700,7701,5432,6379,7702,7703}`. Effective `write_quorum` is **ONE** (starter override, FR-009).

```bash
curl -s http://127.0.0.1:7701/v1/health/ready              # 200
curl -s http://127.0.0.1:7701/metrics | head                # Prometheus text; node_state / buffers present
spacestorage status --output json | jq '.release_profile'   # "first-binary"
```

## 4. PostgreSQL smoke (FR-005, SC-003)

```bash
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'CREATE TABLE t (k int PRIMARY KEY, v text);'
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c "INSERT INTO t VALUES (1, 'a');"
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'SELECT * FROM t;'
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c "UPDATE t SET v = 'b' WHERE k = 1;"
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'DELETE FROM t WHERE k = 1;'
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'DROP TABLE t;'

psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'BEGIN;'
# expected: ERROR 0A000 feature_not_supported (not a silent begin)

psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'COMMIT;'
# expected: ERROR 0A000 (not an empty-transaction notice)

psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'ROLLBACK;'
# expected: ERROR 0A000

psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'COPY t FROM STDIN;'
# expected: ERROR 0A000
```

Credential bootstrap (namespace-bound principal `demo`/`acme`) is `014` / admin CLI; the first-binary tasks wire a documented `users` file analog so this section needs no extra product.

## 5. Redis smoke on K/V Store

```bash
redis-cli -h 127.0.0.1 -p 6379 AUTH <password>
redis-cli -h 127.0.0.1 -p 6379 PING          # PONG
redis-cli -h 127.0.0.1 -p 6379 SET k v
redis-cli -h 127.0.0.1 -p 6379 GET k         # v
redis-cli -h 127.0.0.1 -p 6379 EXISTS k
redis-cli -h 127.0.0.1 -p 6379 SCAN 0
redis-cli -h 127.0.0.1 -p 6379 EXPIRE k 60
redis-cli -h 127.0.0.1 -p 6379 DEL k
redis-cli -h 127.0.0.1 -p 6379 HGET k f
# expected: error (unknown command or not-supported); never a bulk string or nil-as-success
redis-cli -h 127.0.0.1 -p 6379 JSON.GET k
# expected: error (SC-006)
```

## 5b. Document Store as canonical blob (FR-010, SC-002)

Admin-create a persistent `Document Store`. Read/write it over Redis GET/SET (canonical blob) and/or PostgreSQL using the `002` blob mapping. Do not use `JSON.GET` — that must still error.

## 6. Three-node cluster (Story 1 Independent Test)

Stop the one-node process (drain):

```bash
spacestorage stop --wait
```

Prepare data dirs and copy the three fixtures (same token/master/join files). Start A, then B, then C. Admit B and C (`CLUSTER_ADMIN` or one-time token — `011`).

```bash
spacestorage --endpoint 127.0.0.1:7701 topology
# expected: ladder [az], nodes db-a/db-b/db-c, az a/b/c, quorum_domain lab
```

Create a persistent `K/V Store` or `Relational Table` with RF=3 (default anti-affinity `az`).

## 7. Quorum TWO, kill one, restore (SC-001)

```bash
# write at default TWO via psql or redis against any member
kill <pid-of-db-c>                          # crash-stop, not drain
# write again at TWO — must succeed (two durable acks on a,b)
# stop a second data node — TWO must fail

# restart db-c with the same identity and data_dir
# SELECT/GET returns the TWO write (WAL replay, 013)
```

## 8. Refusals that must stay refusals

```bash
# catalog create with multi_active=on → multi_active_unsupported
# entrypoint handler elasticsearch in a running first-binary → never bound; validate fails
```

## 9. Milestone record (SC-004)

A 1–5 ship MUST add `docs/milestones/<nnn>-first-binary.yaml` with deferred slices 6–11 `still_owed: true` ([milestone-record.md](contracts/milestone-record.md)). Reviews that omit that list fail the ledger test.

## Mapping

| Step | Success criteria / stories |
|------|----------------------------|
| 0–3 | SC-001 start; Story 1 sc. 1; one-node write ONE |
| 2, 8 | SC-002 unknown handlers; `multi_active` |
| 4 | SC-003 COPY/BEGIN/COMMIT/ROLLBACK; Story 1 sc. 2 |
| 5 | Story 1 sc. 3; SC-006 extra Redis verbs |
| 5b | Story 1 sc. 4; FR-010 Document Store blob |
| 6–7 | Story 1 sc. 5; Independent Test (THREE-node TWO) |
| 9 | SC-004; Story 2 |
