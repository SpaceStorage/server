# Contract: Conformance profile

**Feature**: `016-mvp-and-nongoals` | Crate: `crates/conformance` | Spec: SC-001–SC-006

## Features

| Cargo feature | What must pass | What must not be required |
|---------------|----------------|---------------------------|
| `first-binary` (default CI) | Gates below | Cassandra/ES/CH/S3/WebDAV smokes |
| `complete-product` | `015` MUST smokes + this file’s gates that still apply | none of the product non-goals |

## Gates (`first-binary`)

| Id | Procedure | Expected |
|----|-----------|----------|
| G1 | `spacestorage validate` + start [one-node fixture](fixtures/first-binary-one-node.conf) | `ready`; `internode` + `replication` bound on loopback; `/metrics` scrapeable; effective `write_quorum ONE`; omitted-transport fixture fails |
| G2 | Bootstrap A; join B and C from three-node fixtures | membership view identical; ladder `[az]` with three values; effective `write_quorum TWO` |
| G3 | `tokio-postgres`: CREATE/INSERT/SELECT/UPDATE/DELETE/DROP; simple + extended | success, auto-commit (one-node uses starter ONE) |
| G4 | `COPY`, `BEGIN`, `COMMIT`, `ROLLBACK` | PostgreSQL `0A000`; no data change |
| G5 | `redis` crate: AUTH, PING, GET/SET/DEL/EXISTS/SCAN, TTL on a KV container | success |
| G5b | `HGET` and `JSON.GET` (and similar off-list verbs) | Redis error; never success or no-op (SC-006) |
| G6 | Default write (TWO) with one node killed | write succeeds (two durable acks); kill a second node → TWO fails |
| G7 | Restart the killed node; WAL replay (`013`) | previous TWO write readable |
| G8 | Entrypoint `handler cassandra` | `entrypoint_unknown_handler` |
| G9 | Catalog create `multi_active=on` | `multi_active_unsupported` |
| G10 | Drain one member (`001`/`011`) | no new tenant connections; in-flight bound by drain timeout |
| G11 | Admin-create `Document Store`; read/write canonical blob via PostgreSQL and/or Redis | success; `JSON.GET` still errors |

Stock CLI tools (`psql`, `redis-cli`) are **quickstart** (SC-001 human path). Automated tests MAY use crate clients.

## Ledger gate (SC-004)

`cargo test -p spacestorage-release-profile --test ledger` reads `docs/milestones/*.yaml`. Any record with `implemented: [1,2,3,4,5]` MUST have `deferred` ids `6,7,8,9,10,11` and `still_owed: true` on each.

## Non-goal audit (SC-005)

`cargo test -p spacestorage-release-profile --test nongoals` greps the spec paths in [non-goals.md](non-goals.md). Missing mention → fail. This is a documentation invariant, not a runtime test.

## Isolation from complete-product CI

Default pipeline job: `--features first-binary` (or default features). A job that enables `complete-product` is a **later** milestone and MUST NOT be a merge gate for slices 1–5.
