# Research: Internode Fabric, Clocks, Quorum Domain, and Conflict Resolution

**Feature**: `012-internode-and-time` | **Date**: 2026-09-18

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-15 and 2026-09-18), constitution 1.3.0, intent `12`, sibling plans `001` (entrypoints), `004` (internodes sketch, quorum arithmetic, repair policy), `006` (Raft on internodes, HLC on metadata), `011` (join secret, replace after FD), `013` (durable WAL), `014` (roles), `016` (first binary).

No `NEEDS CLARIFICATION` remains in Technical Context.

## R1. Three crates: clocks, internodes, replication

- **Decision**: Add `crates/clocks` and `crates/replication`. **Own** `crates/internode` (introduced in `004`). `placement` keeps replica targets, repair **rate/window**, and calls internodes/replication for streams. `clocks` has no I/O.
- **Rationale**: Spec splits coordination (`internode`) from streaming copy (`replication`). HLC is needed by data, metadata (`006`), and skew health without pulling TCP into `controlplane`.
- **Alternatives considered**: One `internode` crate for both ports (rejected: bulk copy vs Raft/heartbeat backpressure); HLC inside `placement` (rejected: `006` would depend on the planner); gRPC (rejected: Principle I).

## R2. Always-on handlers; loopback default; cluster address to join

- **Decision**: Startup **requires** an `internode` entrypoint and a `replication` entrypoint (`001` declaration rule). Omitting either is `internode_required` / `replication_required`. They MUST NOT share a port with each other, `admin`, `admin-http`, or a tenant handler. If `address` is omitted, default **`127.0.0.1`**. `disable internode;` / `disable replication;` are **startup errors** (override `004`). Join to a **remote** seed is refused unless the internodes (and replication) listen address is **not** loopback (`cluster_address_required`). Single-node loopback is valid.
- **Rationale**: FR-001/002; `001` FR-017. Silent `0.0.0.0` would expose the fabric.
- **Alternatives considered**: Implicit undeclared listeners (rejected: `001` FR-016); keep `004` disable (rejected: spec always-on).

## R3. Default ports and transport

- **Decision**: Documented starter ports: internodes **7000**, replication **7001**. Transport on **every** entrypoint: `tls { … }` or `plaintext;` (`001`/`014`). Omitted transport → startup error. First-binary starters use `plaintext;` on loopback. Certs are path refs, never inlined.
- **Rationale**: Cassandra-like internodes port; distinct replication port makes bulk copy obvious (Principle V).
- **Alternatives considered**: Multiplex both on 7000 (rejected: spec two handlers); QUIC (extra deps).

## R4. Frame and version window

- **Decision**: Same envelope on both ports: `u32le length | u8 version | u16le msg_type | payload`. Current version **1**. Accept **1 and 0** (N/N+1 window once version 2 exists: accept N and N−1). Peer whose version is outside the window → close, `version_incompatible`. JSON payload for control and small mutations; **raw length-prefixed bytes** after a stream header on `replication` (repair/bootstrap/source-log batches). Unknown `msg_type` on internodes: `Ack` error `unknown_message`, mesh stays up (`004`).
- **Rationale**: FR-004; `015` mixed-version N/N+1. Reuse `004` codec.
- **Alternatives considered**: protobuf (schema compiler, Principle I risk); separate codecs per port (duplication).

## R5. Auth: join secret now, replication role later

- **Decision**: Before any cluster message: constant-time verify of an accepted join-secret epoch (`011` `cluster.token_file`). Failure → close. First binary: that verify **is** the replication-role check. `014` later binds principal `replication` on the same handshake. Pending joiners may speak **only** `JoinRequest` after secret (`011`). Unauthenticated peers refused (FR-003).
- **Rationale**: Spec + `011` R2. Full RBAC is slice 7.
- **Alternatives considered**: mTLS identity as the only auth (still need secret for join); wait for `014` (misses first binary).

## R6. Message split internodes vs replication

- **Decision**:
  - **`internode`**: Heartbeat (HLC + RTT), membership (`011`), Raft (`006`), CatalogDelta (until Raft), in-domain `FanoutWrite`/`FanoutRead`/`Ack`, 2PC, shuffle **coordination** (`005`), repair **coordination** (who/what), Promote/Fence, Domain gossip.
  - **`replication`**: source-log records to followers, bootstrap/rebuild streams, repair **chunks**, hinted-handoff **replay bytes**.
- **Rationale**: Spec “streaming copy” vs “coordination”. In-domain mutations stay small RPCs like Cassandra internodes.
- **Alternatives considered**: All data bytes on internodes (rejected: Raft vs bulk copy share buffers); all writes on replication (rejected: every `TWO` ack would open a stream).

## R7. `quorum_domain` object and join

- **Decision**: Cluster object `{ name, members[] }`. Bootstrap creates name **`default`** and puts the first node in it. `CLUSTER_ADMIN` `domain-create NAME` before a join can name a second domain. `JoinRequest` **must** include `quorum_domain` (existing name). Omit/unknown → refuse, no pending row (`011`). Each member exactly one domain. Live change refused; move = decommission/replace + join (`011`). Labels never become a domain (`region=eu` ≠ domain `eu` unless an object named `eu` exists).
- **Rationale**: Clarify 2026-09-18 Q1 and Q5; FR-005.
- **Alternatives considered**: Domain as a required ladder-forbidden label (rejected: looks like topology); per-container voting sets (multiple HLCs on one node).

## R8. HLC in-domain only

- **Decision**: `Hlc { physical_micros: u64, logical: u32, node_id: Uuid }` from `crates/clocks`. One `DomainClock` per domain **this node is in** (exactly one). Tick: `max(wall, last.physical)` then logical++. Compare physical → logical → node_id **only if both stamps carry the same `domain_id`**. Cross-domain compare is a programming error / refuse. Persist last tick under `{data_dir}/clocks/<domain>.json` so restart does not go backwards. `006` metadata HLC uses the **controller voters’** domain (first binary: `default`).
- **Rationale**: Spec FR-012; `006` R14. HybridLogicalClock (Demers/Corbett) matches Cassandra-style LWW without trusting raw wall clocks across domains.
- **Alternatives considered**: Vector clocks (size); TrueTime (not our hardware); wall-clock only (skew).

## R9. Skew health

- **Decision**: Heartbeats carry sender HLC. Receiver samples `|physical - local_physical|`. If any peer in the **same** domain exceeds `replication.max_stamp_skew` (default **500 ms**, from `004` R17), set `node_state=degraded` (`unhealthy_clock`) on the observing node and increment skew metrics. **Writes still proceed** (LWW still defined); they MUST NOT be silent. No NTP daemon required; operators MAY run one.
- **Rationale**: FR-014; US3 scenario 3. Exact milliseconds were deferred to plan; 500 ms matches `004`.
- **Alternatives considered**: Refuse writes on skew (over-strict for first binary); require chrony (ops, not product).

## R10. Failure detector

- **Decision**: Heartbeat interval `cluster.heartbeat_interval` default **2 s**. Unavailable after `cluster.failure_timeout` default **15 s** (7.5 missed) since last successful heartbeat. **One cluster-wide value**; live-reloadable. Same event: not counted for write quorum **and** replace-eligible (`011`). Heartbeating `ready`/`draining` still counts. Not phi-accrual. Not per-observer. `004` per-destination-group `failure_timeout` is **removed** for this event (planetary clusters set the **cluster** timeout to minutes, as in `004` long-RTT starter).
- **Rationale**: Clarify Q2; `004` R18 already rejected phi. Replace must not race.
- **Alternatives considered**: Phi (opaque); two-stage suspected/unavailable (extra state `011` did not consume).

## R11. Who counts: source vs follower

- **Decision**: Container has `source_domain` (a `quorum_domain` name) and zero or more `follower_domains[]` (async log-followers). `004` destination **groups** map: sync group → source domain membership ∩ replica targets; async groups → follower domains. Write `ONE`/`LOCAL_ONE`/`TWO`/`QUORUM` count only **durable** replicas in the source (`013`). Followers never count for those write levels. `LOCAL_*` names the **coordinator’s** domain. `LOCAL_ONE` **write** always = one durable WAL in the **source**. `EACH_QUORUM` = source quorum + apply of that **source log position** in each opted-in follower, or refused (`004` wait policy).
- **Rationale**: FR-006–009; `004` FR-028/032/074 after 2026-09-15 clarify.
- **Alternatives considered**: Count followers for `TWO` (rejected); `LOCAL_ONE` write as follower WAL (rejected in specify clarify).

## R12. Forward, fail, fallback

- **Decision**: Write received in source A: leaderless fan-out in A (`004` R19). Write received in follower B: **forward to A** (internodes to a source-domain coordinator/replica). If A cannot meet the **requested** level: **fail**. Session/query MAY set `quorum_fallback=LOCAL_ONE` (`002`/`004` option). Fallback still needs one durable WAL in A; response names the level met. If even that fails: fail; B MUST NOT local-WAL. No auto-promote. Ordered/log types: forward to leader in A or error (`006` lease).
- **Rationale**: Clarify Q4; FR-007/008.
- **Alternatives considered**: Hint+ack on B (second source log); auto-promote (contradicts manual promote).

## R13. Source log and in-domain LWW

- **Decision**: Each container has a **source log** (monotonic `u64` position, assigned in A, durable with the WAL). Followers apply in position order; last source position wins; follower local HLC MUST NOT win. In-domain concurrent writes: LWW by HLC stamp (`004` FR-037); type-supported deterministic merge optional. Disagreement recorded (metrics + admin `repairs`); stale replicas corrected via internodes coordination + replication bytes. Concurrent writes never wait for a human. Ordered types use `006` leadership, not LWW, to repair two histories.
- **Rationale**: FR-012/013; constitution XII.
- **Alternatives considered**: Retain both values as conflicts (product non-goal).

## R14. Promote and fence

- **Decision**: `Promote { container, to_domain, force: bool, accept_data_loss: bool }` CLUSTER_ADMIN. Ordinary (`force=false`): allowed if follower applied through last **known** source position, **or** every member of the old source is FD-unavailable. Otherwise refuse. Force: requires `accept_data_loss=true`; still allowed when A is live and B is lagging. Success: `epoch += 1`, `source_domain = to_domain`, old domain fenced at old epoch. Old source MUST NOT accept writes until it rejoins as follower. Two live sources protocol-refused. No live domain membership change for **nodes** (R7); this is a **container** source switch.
- **Rationale**: Clarify Q3; FR-010.
- **Alternatives considered**: Promote anytime (silent data loss); no force path (cannot recover if A is gone and catch-up cannot be proven against a live head — ordinary path already allows FD-unavailable).

## R15. `multi_active` and first binary

- **Decision**: Catalog flag on container/type, **default off**. Ordered/log types forced off. Create with `on` → `MultiActiveRefused` in the first binary (`016`). Dual-active merge not shipped until a non-HLC merge exists.
- **Rationale**: FR-011; `016` non-goal for first binary.
- **Alternatives considered**: Silently ignore the flag (lying); ship dual-active LWW across domains (spec forbids HLC compare).

## R16. RTT override, backpressure, buffers

- **Decision**: Internodes heartbeats measure RTT; EMA published to `004`/`005` to override ladder rank once samples exist. Backpressure: bounded `internode.send`/`internode.recv` and `replication.send`/`replication.recv` (default 64 MiB each, live-reloadable). Full buffer → slow or fail **that stream**; never `std::thread::park` a worker. Test delay directive unchanged (`004` R20).
- **Rationale**: FR-018/019; constitution II.
- **Alternatives considered**: Global stop-the-world when one stream is full (rejected).

## Technical Context resolution

| Item | Resolution |
|------|------------|
| Language | Rust 1.87 / MSRV 1.85 |
| Dependencies | tokio, serde, in-house HLC/frames; no gRPC |
| Storage | ClusterStore for domains/epochs; `003` WAL for source log; local clock file |
| Testing | cargo test + in-process conformance |
| Platform | Linux primary; macOS dev; loopback mesh |
| Project type | `clocks` + `replication` new; own `internode` |
| Performance | loopback heartbeat < 5 ms p95; no follower RTT on source writes |
| Constraints | always-on, loopback default, N/N+1, cluster-wide FD, no follower WAL |
| Scale | first binary 1- and 3-node one domain |
