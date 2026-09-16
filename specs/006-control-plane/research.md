# Research: Control-Plane Hierarchy, Raft Elections, and Node Restore

**Feature**: `006-control-plane` | **Date**: 2026-09-16

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-16), constitution 1.3.0, intent `06`, sibling plans `004` (`ClusterStore` seam), `011` (membership), `012` (internode, HLC), `013` (restore/WAL), `008` (election series), `016` (slices; amended by FR-020).

## R1. One crate implements `ClusterStore`

- **Decision**: Add `crates/controlplane` (`spacestorage-controlplane`). It implements the `004` `ClusterStore` trait (append + snapshot + apply). Placement, membership (`011`), and 2PC keep calling `ClusterStore`; the **shipper** changes from internodes `CatalogDelta` LWW to Raft commit. Event/log record types stay (`PlacementEvent` plus cluster/namespace control records).
- **Rationale**: `004` R3 promised this seam. Putting `openraft` inside `placement` would pull Raft into every `003` unit test.
- **Alternatives considered**: Grow `placement` (rejected: dep graph); new `etcd` process (rejected: Principle III); keep LWW until slice 7 (rejected: FR-020 / SC-003).

## R2. Raft library is `openraft`

- **Decision**: Use **`openraft`** (Tokio, pure Rust) with a custom `RaftNetwork` (internodes) and `RaftLogStorage`/`RaftStateMachine` (files under `data_dir/raft/`). Workspace pins a version that builds on MSRV 1.85 without `*-sys`.
- **Rationale**: Joint configuration, snapshots, and leader lease are already solved. Matches constitution I/II.
- **Alternatives considered**: `raft` (tikv, more sync/batch-oriented; extra work to not block workers); hand-rolled majority (misses log matching); `async-raft` 0.6 (predecessor of openraft).

## R3. Raft RPCs on existing `internode`

- **Decision**: Additive internodes `msg_type`s (unknown type already ignored per `004` coordinator contract): `RaftVote`, `RaftAppend`, `RaftSnapshot`, `RaftForward` (client write to a follower), `MetricsPush`. Same length-prefixed frame, same TLS/plaintext and replication-role auth (`012`). No new port.
- **Rationale**: FR-014; Principle V.
- **Alternatives considered**: Dedicated `raft` handler/port (rejected: extra entrypoint, spec says internodes); gRPC (rejected: Principle I).

## R4. First-binary voter set stays odd without +1 steps

- **Decision**: Bootstrap: voter set = `{first node}` (size 1). Second member is a **learner/non-voter**. When the **third** member is admitted, one configuration change expands `{A}` → `{A,B,C}`. Further first-binary joins are non-voters. Slice 7: **replace** is one config change swapping one id (size unchanged); **grow** is +2; **shrink** is −2. A step that would make the committed voter set even, or lose majority, is refused (`FR-018`).
- **Rationale**: Spec requires odd size at every accepted step, so classic 3→4→3 add-then-remove is illegal. `016` topologies are 1-node and 3-node.
- **Alternatives considered**: All members vote (rejected: clarify Q1); 3→4 joint then remove (rejected: even intermediate); two-node both voting (rejected: even).

## R5. Namespace groups and learners

- **Decision**: Creating a namespace allocates `GroupId::Namespace(id)` and starts a Raft group whose **initial voter set copies the cluster voter set**. Other **members** that need definitions (every member, because any node may coordinate) are **learners** of that group. Cluster group: all members are learners if not voters. Learners receive the log; they do not vote (`FR-002`, `FR-010`).
- **Rationale**: Non-voters must restore definitions (`FR-009`) and serve metadata-consistent coordination. A second copy protocol would fork `CatalogDelta`.
- **Alternatives considered**: Definitions only on voters (rejected: isolated data node cannot restore); one Raft for all namespaces (rejected: clarify Q2 option C); Raft per container (rejected: clarify Q2).

## R6. Durable Raft log, not tenant WAL

- **Decision**: `{data_dir}/raft/<group_id>/log/` and `snapshot/` with a **format version** on every file (`013`/`015` N/N+1). Fsync via `spawn_blocking`. Tenant WAL (`013`, one stream per node/drive) is unchanged. Controller ack for a metadata write means the **Raft log entry is durable on a majority of that group's voters**.
- **Rationale**: Mixing schema entries into tenant WAL would couple compaction/`gc_grace` to membership. Separate dirs isolate corruption (`spec` edge: controller still up if metadata intact).
- **Alternatives considered**: Shared tenant WAL (rejected: encryption-per-container vs cluster roles); in-memory Raft only (rejected: restore / constitution XII).

## R7. What lives in which log

- **Decision**: **Cluster log**: cluster UUID/name (from `011`, not regenerated), membership view, node identities and addresses, drive and memory inventory pointers, namespace **list** (id, name, voter-set id), **role blob** (`07`/`14` records opaque to this crate), cluster voter set, `controller_exclusive_data` flag (slice 7). **Namespace log**: schemas, container definitions and options, shared-datatype metadata, leadership leases. Placement events (`004`) that are cluster-wide (node join, topology) apply from the cluster log; per-container placement apply from the namespace log (or stay as `PlacementEvent` with a `namespace_id` so `ClusterStore::append` routes to the right group).
- **Rationale**: FR-003 vs FR-004. Routing `PlacementEvent` by whether it names a container keeps one trait.
- **Alternatives considered**: All placement in cluster log (rejected: namespace minority would still mutate schemas if they were cluster keys — actually they'd be cluster majority; rejected by FR-003); dual write (rejected: two truths).

## R8. Leadership lease and epoch fence

- **Decision**: `LeadershipLease { container_id, holder: NodeId, epoch: u64 }` is a namespace-log record. Grant/steal increments `epoch`. Ordered-type appends (and `004` FR-078 writes) **carry `epoch`**. Replica and lease holder refuse `epoch < current`. New grant is allowed only if the namespace group has majority. No wall-clock-only fence. First binary: API exists; no first-binary type takes a lease (`FR-020`). Cargo: compile `lease.rs` always; conformance SC-006 behind ordered-type / `controlplane-leases`.
- **Rationale**: Clarify Q4; FR-006. LWW MUST NOT merge two ordered histories.
- **Alternatives considered**: Namespace primary is always the log writer (rejected: couples throughput to metadata leader); timeout-only (rejected: partitioned holder keeps writing).

## R9. Restore orchestration

- **Decision**: On start, before `ready`: (1) open local Raft stores for groups this node is voter or learner of; (2) apply snapshots+logs to catalog; (3) call `013` restore for persistent/hybrid **content**; (4) leave memory-mode **content** empty; (5) if replicated memory-mode, `004` may catch up after internodes is up; (6) if this process is **not** in the cluster membership view, do not start Raft as a voter and do not accept new replica placements (`FR-010`). Definitions/options come from applied namespace state (plus local catalog files that must match). Volatility notice is a container description flag from `003`/`013`.
- **Rationale**: FR-009; constitution XII. This feature invokes `13`, it does not reimplement WAL.
- **Alternatives considered**: Restore content from Raft snapshots (rejected: tenant data is not in the Raft log); vote before membership (rejected: FR-010).

## R10. Metadata reads on secondaries

- **Decision**: Followers serve metadata reads after **read-index** (or wait until applied ≥ leader commit). SC-007 requires match with primary — not stale-by-default. Writes on a follower: **forward** to the leader if known, else **redirect** `NotLeader { leader: Option<NodeId> }` (`FR-011`). Never apply a write only on the follower.
- **Rationale**: Load-balancing reads without split-brain writes.
- **Alternatives considered**: Always stale follower reads (fails SC-007); clients must hit the primary (rejects load-balancing MUST).

## R11. Slice 7 operations

- **Decision**: Feature `controlplane-ops`: admin/CLI `controllers voters replace <group> --from <node> --to <node>` (atomic swap), `controllers voters grow/shrink` by 2, and `cluster { controller_exclusive_data on; }` (reload live). Placement reads a `NodeFlags::no_tenant_data` for every current cluster or namespace **voter**. Enabling the flag while tenant replicas remain: **block** until `04` rebalance/`11` drain removes them; do not drop. First binary ignores the directive if present? **No** — first binary **must not implement on**; `validate` accepts `off` or omission; `on` is `unknown_directive` or `slice7_required` until the feature is compiled. Default off.
- **Rationale**: FR-018/019/020.
- **Alternatives considered**: Ship migration in first binary (rejected: FR-020); drop replicas when exclusive turns on (rejected: spec).

## R12. Exclusive-data and `04`

- **Decision**: `controlplane` publishes `fn tenant_replica_excluded(node: &NodeId) -> bool`. `004` planner treats true like a selector miss (`excluded: controller_exclusive_data`). Rebalance resume (`004` FR-068) already handles controller restart.
- **Rationale**: Spec out of scope for quorum arithmetic except this flag.
- **Alternatives considered**: Duplicate anti-affinity key `controller` (rejected: optional and off by default; operators did not declare a ladder key).

## R13. Shared-datatype metric aggregation

- **Decision**: Members **push** local shared-datatype figures to the **namespace primary** over `MetricsPush` (interval = `cluster.raft.heartbeat`). Primary holds an in-memory map `(datatype_id → merged stats)` and a `stale: bool` if it has not heard from a replica within `2 * interval`. Cluster primary does not merge. Lease holder does not merge. `/metrics` on the primary node exposes the merged series; other nodes expose local only. Names: `08`.
- **Rationale**: Clarify Q5; FR-013.
- **Alternatives considered**: Scraper join (rejected: constitution “stored in memory” on the primary); cluster-wide merge (rejected: tenancy mix).

## R14. HLC on metadata vs Raft index

- **Decision**: Every applied cluster/namespace record carries `hlc: Hlc` from `012` **in the source quorum_domain of the controller voters** (first binary: one domain). Raft `(term, index)` is the commit order. Clients/admin show HLC as the metadata version (`FR-015`). Do not LWW-compare metadata across a partition; Raft is the truth.
- **Rationale**: Spec wants HLC stamps; using HLC instead of Raft would reintroduce the `004` interim shipper.
- **Alternatives considered**: HLC-only metadata (rejected: FR-007); Raft index as the only version shown to operators (rejected: FR-015).

## R15. Election metrics

- **Decision**: This crate increments the `08` system figures: leader elections total, errors total, histogram of leader election duration. Labels: `node`, `raft_group` (`cluster` \| namespace name). Do not rename. Exposition remains `08`.
- **Rationale**: FR-016; `08` FR-009.
- **Alternatives considered**: Private metric names (rejected: observability contract).

## R16. Timeouts do not assume a short RTT

- **Decision**: Documented defaults: `cluster.raft.heartbeat 500ms; election_timeout 2s;`. Conformance loopback fixtures set `heartbeat 50ms; election_timeout 300ms;`. Defaults MUST be overridable. MUST NOT hardcode sub-100ms production timeouts (`004` FR-073 family).
- **Rationale**: Planetary / high-delay domains still elect; tests stay fast.
- **Alternatives considered**: etcd 150ms default (assumes LAN); tie election to internodes `failure_timeout` 15s (too slow for SC-002 on loopback without fixtures).

## R17. `016` sequencing amendment

- **Decision**: First shippable binary **includes** cluster Raft and per-existing-namespace Raft (`FR-020`). Slice 7 in the ledger becomes: voter migration, exclusive-data, `007` quotas, `014` full vocabulary — **not** “introduce Raft”. Update `016` contracts/slices when that feature is next edited; this plan is the source of the tightening.
- **Rationale**: Clarify Q3. Interim LWW cannot implement minority refuse.
- **Alternatives considered**: Follow `016` plan Complexity Tracking literally (rejected: user chose B).

## R18. Role store is opaque bytes

- **Decision**: Cluster state includes `roles: Vec<u8>` or a documented JSON map **without** interpreting role names. `07`/`14` serialize; this crate replicates. Empty on bootstrap except whatever `14` first-binary master-key wrap needs (KEK pointers may live here as opaque keys).
- **Rationale**: FR-012; out of scope for RBAC vocabulary.
- **Alternatives considered**: Hardcoding `admin`/`replication`/`custom` here (rejected: `07` inventory).

No `NEEDS CLARIFICATION` remains in Technical Context.
