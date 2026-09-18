# Quickstart: Internode fabric, clocks, source vs follower

**Feature**: `012-internode-and-time` | **Gates**: SC-001–SC-009 | First binary slices 1–5 (`016`)

Prerequisites: `001` admin token; `011` bootstrap/join secret; ladder `[az]` (`004`/`016`). Model: [data-model.md](../data-model.md). Commands: [admin-cli.md](contracts/admin-cli.md).

Configs: [single-node.conf](contracts/fixtures/single-node.conf), [three-node-a.conf](contracts/fixtures/three-node-a.conf), [two-domain-source.conf](contracts/fixtures/two-domain-source.conf).

## 1. Single node always listens (SC-001, SC-008)

Start [single-node.conf](contracts/fixtures/single-node.conf). Expect: `ready`; internodes `:7000` and replication `:7001` accept on `127.0.0.1`; `spacestorage domains` shows `default` with one member; `spacestorage fabric` lists both handlers.

[omit-transport.conf](contracts/fixtures/invalid/omit-transport.conf) → startup fail. [omit-internodes.conf](contracts/fixtures/invalid/omit-internodes.conf) → startup fail.

Connect without join secret → refused.

## 2. Three-node source-domain quorum (SC-002, SC-009)

Bootstrap `db-1`, admit `db-2`/`db-3` into `quorum_domain default` with non-loopback internodes addresses. Write at default `TWO`. Expect: two durable **source** acks; kill one node; after 15 s it does not count and replace is allowed (`011`); it MUST NOT count toward `TWO`.

[loopback-join.conf](contracts/fixtures/invalid/loopback-join.conf) → `cluster_address_required`.

## 3. Follower forward and fallback (SC-003, SC-004)

Create domain `us`, join a follower, place async replicas (`004` Story 7). Write in `us` at `QUORUM` with no fallback → forwarded; if `eu` cannot meet it → fail; no local WAL on `us`. Same write with session `quorum_fallback=LOCAL_ONE` → succeeds only if one durable WAL in `eu`; response `met=LOCAL_ONE`. Cut `eu` entirely → even fallback fails. `LOCAL_ONE` **read** in `us` MAY be stale local apply.

`multi_active=on` create → `MultiActiveRefused` (SC-005).

## 4. HLC vs source log (SC-006)

Two concurrent writes in `default` → converge on later HLC (or type merge); conflict recorded. Value on a follower vs source → follower applies source-log position; follower HLC MUST NOT win. Inject skew > 500 ms in-domain → `node_state=degraded`; writes still succeed.

## 5. Promote (SC-007)

Caught-up follower: `spacestorage promote <c> --to us` → new epoch; old source fenced. Lagging follower while source still heartbeats: ordinary promote refused. `--force` without `--accept-data-loss` refused. With accept: succeeds and fences. Two live sources occur in 0 tests.

Live `domain-assign` of a member → refused; decommission/replace then join (SC-008).
