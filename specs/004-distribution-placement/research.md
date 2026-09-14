# Research: Distribution, Placement, Media, and Replication

**Feature**: `004-distribution-placement` | **Date**: 2026-09-14

Resolves every Technical Context item and every planning default left open by the spec. No `NEEDS CLARIFICATION` remains.

---

## R1. Crate decomposition and the `003` seam

**Decision**: Grow `crates/placement` into the engine; add `crates/internode` as the only new crate. Keep `PlacementDirector` and `PlacementInfo` signatures from `003`/`002`; add methods with default bodies.

**Rationale**: `003` already routes validated `CapabilityDecl`s into `PlacementDirector`. A second capability crate would recreate the parallel hierarchy the constitution forbids. Internodes is a `001` `Handler` and must not pull TCP into the type-system graph.

**Alternatives considered**: New `crates/cluster` owning both placement and RPC (rejected: `003` would take a new dependency and `LocalDirector` tests would drag internodes). gRPC internodes (rejected: Principle I).

---

## R2. Topology source of truth

**Decision**: Each node declares *itself* in config: `node { name; labels { k v; } }`, `storage { drive NAME { path; media; size?; labels { } } }`, `memory { size; labels { } }` (size already in `003`). Cluster membership is `cluster { peers { name; address; port; } locality_key region; }`. Live mutations (add/change/remove a label, add a drive, resize memory) go through admin/CLI and are written to the placement log; they do not require a restart except `node.name` and internodes bind address.

**Rationale**: Spec FR-001–FR-010 need a queryable view and live label changes. Config is the bootstrap; the log is the live view. `003` already owns `storage.data_dir` and `memory.size`; this feature adds `drive` sub-blocks and treats `memory.labels` as placement inputs.

**Alternatives considered**: Central topology service (`06` will be that). Cloud instance-metadata auto-labels (out of scope; labels stay operator-declared).

---

## R3. Cluster metadata before Raft (`06`)

**Decision**: `ClusterStore` trait: append-only log of `PlacementEvent`s (node join/leave, label change, placement, replica health, rebalance step, 2PC record) + snapshot. Each node applies locally and ships deltas to peers over internodes (`CatalogDelta`). Last-writer-wins per event id using the same HLC as data. `06` replaces the shipper with Raft; the log format stays.

**Rationale**: Same pattern as `003`'s node-local `CatalogStore`. Inventing Raft here violates Principle XII.

**Alternatives considered**: Shared filesystem (not planetary). External etcd (not in-process, not Rust-only as a product surface).

---

## R4. Placement algorithm

**Decision**: Deterministic greedy: (1) exclude nodes failing the selector or missing the anti-affinity key; (2) exclude nodes without matching media capacity or memory room; (3) order remaining candidates by `(anti-affinity distinctness, least loaded, node_name)` where load is `used_bytes / capacity` then request-rate EMA; (4) pick `factor` nodes such that each new pick maximises distinct values of the anti-affinity keys in order. Seed nothing random — equal load ties break on `node_name` lexicographic. Same inputs ⇒ same plan on every node (FR-016). Unsatisfiable ⇒ refuse, nothing allocated (Q5, FR-021).

**Rationale**: Spec requires inspectable reasons for every exclusion and forbids silent downgrade. Greedy is explainable in the placement report; CRUSH-like straw would hide why a node was skipped.

**Alternatives considered**: Random among feasible (non-deterministic). Min-cut / ILP (overkill, not explainable). Kubernetes-style scheduler scores (heavier, no win at this scale).

---

## R5. Label selector language

**Decision**: Expression over node, drive, and memory labels:

```text
sel     := term ('and' term)* | term ('or' term)*
term    := 'not' term | '(' sel ')' | atom
atom    := KEY '=' VALUE | KEY '!=' VALUE | KEY 'in' '(' VALUE (',' VALUE)* ')'
         | KEY 'notin' '(' ... ')' | 'has' KEY | 'media' '=' MEDIA | 'memory'
```

`media=nvme` matches a node that has at least one drive of that kind; placement then pins the replica to a specific matching drive. `memory` matches a node with a declared pool that has room. No regex. Keys and values are case-sensitive IDENT/STRING.

**Rationale**: Covers FR-011 (equality, inequality, set, presence, and/or) without becoming a programming language. `media` and `memory` are sugar over derived labels (`media=nvme` ≡ `has media_nvme` plus capacity check).

**Alternatives considered**: JSONPath / CEL (too much). Kubernetes label selectors only (no boolean `or`, no media sugar).

---

## R6. Quorum arithmetic and durable acknowledgements

**Decision**: Implement `002`'s vocabulary here:

| Level | Acknowledgements required |
|-------|---------------------------|
| `ONE` / `LOCAL_ONE` | 1 (locality domain for `LOCAL_*`) |
| `TWO` / `THREE` | 2 / 3 |
| `Acks(n)` | n |
| `QUORUM` | `floor(n/2)+1` of the replica set (or shard) |
| `LOCAL_QUORUM` | `floor(local_n/2)+1` in the requester's locality domain |
| `EACH_QUORUM` | `LOCAL_QUORUM` in **every** destination group that holds replicas |
| `ALL` | every replica in the replica set |

Precedence: query → session → container (`capability.quorum.write/read`) → namespace (`07`) → global `query_defaults`. Explicit unsatisfiable ⇒ reject; default-sourced ⇒ clamp (already `002`).

**Durable filter (Q3)**: for persistent and hybrid containers, only acknowledgements from replicas whose copy is on a drive count toward **write** quorum. Memory replicas may be contacted and are listed in the execution record as `ack_kind: memory`. Memory-mode containers count memory acknowledgements. Reads count any in-sync replica of the container's mode.

Worked `QUORUM` sizes: RF=2 → 2; RF=3 → 2; RF=4 → 3; RF=5 → 3 (FR edge case).

**Rationale**: Constitution VIII + Clarification Q3. Counting a memory ack on a persistent container would make SC-008 ("0 acknowledged writes lost") false.

**Alternatives considered**: Equal counting with a durability footnote (rejected in clarify). Requiring a persistent replica even for memory-mode (makes memory-mode unwritable at `TWO`).

---

## R7. Version stamps (LWW)

**Decision**: Hybrid logical clock stamp `(physical_micros: u64, logical: u32, node_id: NodeId)`. Compare physical, then logical, then `node_id` bytes. Coordinators assign stamps at fan-out; clients may supply a timestamp only if it is not in the future beyond `max_stamp_skew` (default 500 ms) — otherwise the coordinator stamp wins. Skew between peers is a gauge; above `max_stamp_skew` the node is `unhealthy_clock` and is excluded from new placements until it catches up. Type-supported alternatives (e.g. G-counter merge) are a `ConflictMerge` fn on the type descriptor; default is LWW.

**Rationale**: Clarification Q2. Raw wall clocks fail under skew; vector clocks explode with RF and do not match Cassandra-style clients. HLC is bounded and total-ordered.

**Alternatives considered**: Cassandra client timestamps only (hostile to protocols without a timestamp field). TrueTime (not available). Keep-all conflicts (rejected in clarify).

---

## R8. Destination groups and sync/async

**Decision**: A container's replication declaration expands to one or more `DestinationGroup`s: `{ selector, factor, mode: sync|async, lag_threshold }`. Default when only `factor` + `anti_affinity` is given: one synchronous group over the whole topology. Multi-region example: `{ selector: region=eu, factor: 3, mode: sync }` + `{ selector: region=us, factor: 2, mode: async, lag_threshold: 60s }`.

Synchronous groups participate in write quorum (FR-039). Asynchronous groups receive the write after the coordinator has acknowledged, and expose lag (FR-041). Reads at `LOCAL_*` prefer the requester's group.

**Rationale**: Spec FR-023 (per-label-value counts) and FR-038 (mode per destination group). One group is the simple path; two groups are Story 7.

**Alternatives considered**: Per-replica mode flags (harder to reason about `EACH_QUORUM`). Always-sync with "later" as a client retry (does not give cheap local writes).

---

## R9. `EACH_QUORUM` vs asynchronous remote (Q4)

**Decision**: Default `each_quorum_policy: refuse` on a container that has any async group. Requests stating `EACH_QUORUM` or `ALL` that would require an async group's acknowledgement fail before internodes wait, with code `QuorumRequiresAsyncGroup`. Container may set `each_quorum_policy: wait`, which temporarily treats async groups as sync for that request (and for `ALL`). Queued-send-as-ack is forbidden.

**Rationale**: Clarification Q4 + FR-072 (local write must not wait). Waiting by surprise would break SC-010.

**Alternatives considered**: Wait-by-default (rejected). Satisfy on enqueue (a lie).

---

## R10. Hinted handoff and repair

**Decision**: Coordinator stores missed writes for an unavailable replica as **hints** on a healthy replica in the same group, retained `hinted_handoff_window` (default **3 h**, configurable per cluster and per group). On return, hints replay; if the window elapsed or the replica's last-applied stamp cannot be ordered, **full compare** against a healthy replica (Merkle-tree of stamp+hash per key range). Background anti-entropy every `repair_interval` (default **24 h**) at `repair_bytes_per_sec` (default **32 MiB/s** per node).

**Rationale**: Cassandra-proven. 3 h is a planning default that is not a low-RTT assumption (FR-073); interplanetary groups raise the window in the starter example.

**Alternatives considered**: Infinite hint log (disk blow-up). Repair-only (slow catch-up for brief blips).

---

## R11. Sharding

**Decision**: Default scheme `hash` using rendezvous hashing (HRW) over `shards` virtual nodes (default 16 when `capability.sharding.shards` omitted but sharding is declared). Key is the declared field path(s), canonical-encoded. Each shard is its own replica set with the container's RF, anti-affinity and media. Resharding adds virtual nodes and moves the affected key ranges; at every moment a key maps to exactly one shard (FR-052). Imbalance threshold: a shard holding **> 2×** the median shard bytes or request rate is a rebalance candidate (FR-056).

**Rationale**: Rendezvous hashing minimises movement when the shard count changes compared with modulo. Range sharding is available as `partitioning.scheme=range` instead of overloading hash.

**Alternatives considered**: Consistent-hash ring with vnodes (more moving parts). Cassandra token ranges (requires vnode ops the control plane does not have yet).

---

## R12. Partitioning

**Decision**: `scheme: range | time` (hash partitioning is sharding). Range: operator- or auto-split key ranges, each a replica set; splits at a configured size (`partition_split_bytes`, default 8 GiB). Time: `interval` (e.g. `7d`) on a timestamp field; new partitions created on first write in the bucket; old partitions may declare a different selector (NVMe → HDD) via `partition_policy`. Present as one container.

**Rationale**: FR-051. Keeps hash in the sharding capability so the two declarations cannot silently mean the same thing (`003` already refuses conflicting keys).

**Alternatives considered**: Unified "distribution" capability (would collapse L1 inventory the spec lists separately).

---

## R13. Rebalance planner

**Decision**: Produce an ordered list of `Move { replica_or_shard, from, to, bytes }`. Invariant: after every prefix of the list, anti-affinity holds **or** a declared temporary violation is attached to that step with `max_duration`. Never drop below the container's write-quorum availability (extra replica is created before the old one is removed — "add then remove"). Throttle `rebalance_bytes_per_sec` (default 64 MiB/s per node), pausable, resumable from a `plan_id` + `cursor` in the placement log. Interrupted coordinator: any peer resumes from the log (FR-068).

**Rationale**: Spec FR-065–FR-070. Add-then-remove is the only way SC-013 can claim "0 intervals unavailable at declared quorum".

**Alternatives considered**: Remove-then-add (violates RF during the hole). Kubernetes-style eviction (not data-aware).

---

## R14. Distributed transactions

**Decision**: Per-request 2PC. The receiving node is the transaction coordinator (leaderless data path — it is not a sticky leader). Participants are the shards/replica-sets the keys map to. Prepare is itself a quorum write of a `TxnPrepare` record on each participant's replica set; Commit/Abort likewise. Participant state durable in the placement/catalog log. In-doubt transactions are visible (`state: prepared`) and recovered by any node on timeout (`txn_timeout`, default 30 s, configurable per group). Isolation and SQL syntax remain `05`.

**Rationale**: FR-076/077 without running Raft. 2PC fits "atomic commit across participants"; Percolator/Calvin need a timestamp oracle that is `06`.

**Alternatives considered**: Raft per shard (that's `06` + more). Best-effort multi-key without atomicity (fails SC-015).

---

## R15. Internode protocol

**Decision**: New handler `internode` on its own entrypoint (must be enabled or `disable internode;`, same `001` rule as admin). Length-prefixed frames: `u32le length | u8 version | u16le type | payload`. Payload is serde JSON for v1 (debuggable, enough for metadata and small mutations; bulk repair streams a follow-on length-prefixed byte channel). Auth: `cluster { token_file P; }` compared constant-time; optional `entrypoint.tls` as in `001`. Message kinds: `Heartbeat`, `CatalogDelta`, `FanoutWrite`, `FanoutRead`, `Hint`, `RepairBegin/Chunk/End`, `RebalanceChunk`, `TxnPrepare/Vote/Commit/Abort`, `Ack`.

**Rationale**: Distinct port keeps internodes operationally obvious (Principle V). JSON v1 avoids a schema compiler; repair bulk is raw bytes so we are not encoding SSTable blocks as JSON.

**Alternatives considered**: Multiplex on admin port (confuses ACL and drain). QUIC (young, extra deps).

---

## R16. Configuration blocks

**Decision**: Claim reserved words from `001`: `labels` (nested in `node` and `drive`/`memory`), `cluster`, `replication`. Extend `003`'s `storage` with repeatable `drive NAME { path; media; size?; labels{} }` and keep `memory { size; labels{} }`. New buffers: `placement.hints`, `placement.repair`, `internode.recv`, `internode.send`. See [contracts/config-directives.md](contracts/config-directives.md).

**Rationale**: Spec assumption that those reserved names belong here.

**Alternatives considered**: YAML sidecar for topology (second config language).

---

## R17. Operational defaults (FR-073)

None of these assume a LAN RTT; interplanetary starter overrides them.

| Knob | Default | Notes |
|------|---------|-------|
| `cluster.locality_key` | `region` | |
| `cluster.heartbeat_interval` | `2s` | internodes |
| `cluster.failure_timeout` | `15s` | 7.5 missed heartbeats |
| `replication.hinted_handoff_window` | `3h` | per group override |
| `replication.repair_interval` | `24h` | |
| `replication.repair_bytes_per_sec` | `32m` | per node |
| `replication.rebalance_bytes_per_sec` | `64m` | per node |
| `replication.async_lag_threshold` | `60s` or `10000` writes, either trip | |
| `replication.max_stamp_skew` | `500ms` | |
| `replication.txn_timeout` | `30s` | |
| `query_defaults` | write `TWO`, read `ONE` | unchanged from `002` |

**Rationale**: Documented, overridable, not compiled-in. The minutes-RTT example sets `failure_timeout 30m`, `hinted_handoff_window 168h`, `txn_timeout 1h`.

---

## R18. Failure detection

**Decision**: Fixed timeout over internodes heartbeats (table above), not phi-accrual, because phi-accrual adapts to observed RTT and would silently stretch on a slow interplanetary link in ways operators cannot read off config. The timeout **is** the operator-visible SLA. A peer that misses `failure_timeout` is `unreachable`; replicas on it go `degraded`. Permanent removal is an explicit `decommission` admin op (FR-064), never automatic.

**Rationale**: FR-057 configurable interval; FR-073 explicit knobs. Phi-accrual is a better LAN detector but hides the threshold.

**Alternatives considered**: Phi-accrual (rejected for opacity). SWIM gossip (extra protocol).

---

## R19. Leaderless data path vs datatype primary (`06`)

**Decision**: Confirmed Q1. `FanoutWrite` is sent to every replica in the target replica set (or to a subset large enough to meet quorum, always including enough durable ones). No replica serialises writes. `06`'s datatype-level primary owns shared-type *metadata* (schema catalog leadership, not per-key). Types that set `leader_election` compatible and declare it get a `LeaderFor(container)` from `06`; internodes then forwards writes to that leader — the exception path of FR-078.

**Rationale**: Constitution VI + VIII together are Cassandra, not Raft-per-key.

**Alternatives considered**: Always a write leader (rejected in clarify).

---

## R20. Conformance harness

**Decision**: In-process cluster: N `spacestoraged` runtimes on ephemeral ports, shared temp `data_dir`s, internodes meshed. Fixtures from `contracts/fixtures/` copied per node. Latency injection: `internode { delay { to us-1 200ms; } }` test-only directive (compiled in `cfg(test)` / `conformance` feature, rejected in production config as `unknown_directive` unless `SPACESTORAGE_TEST=1`). Catalog of tests maps 1:1 to SC-001–SC-018.

**Rationale**: Same in-process pattern as `002`. Real multi-machine is not required to prove the contracts.

**Alternatives considered**: docker-compose in CI (slower, harder on macOS).

---

## Technical Context resolution

| Item | Resolution |
|------|------------|
| Language | Rust 1.87 / edition 2024 / MSRV 1.85 |
| Dependencies | Workspace crates only; no new `*-sys` |
| Storage | `003` catalog + `catalog/placement|hints|repair`; user data unchanged |
| Testing | `cargo test` + multi-node in-process conformance |
| Platform | Linux servers, macOS dev |
| Project type | Workspace libraries, no new binaries |
| Performance | See plan Technical Context |
| Constraints | Leaderless, durable acks, strict anti-affinity, async internodes |
| Scale | 13 capabilities, ~25–35 k lines |
