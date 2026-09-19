# Feature Specification: Internode Fabric, Clocks, Quorum Domain, and Conflict Resolution

**Feature Branch**: `012-internode-and-time`

**Created**: 2026-09-15

**Status**: Draft

**Input**: User description: "Read .specify/intent/12-internode-and-time.md and specify this feature." — dedicated `internode` and `replication` handlers always listening; explicit `quorum_domain`; source vs log-follower write/ack/read/promote; HLC in-domain only; LWW in-domain; source-log across domains; `multi_active` default off and create-on refused in the first binary; failure detection crash-stop not Byzantine.

## Clarifications

### Session 2026-09-15

- Q: Is `planet` an HLC or write-quorum domain? → A: Never. `planet` is a topology-ladder label (`04`). Voting/HLC set is an explicit `quorum_domain`.
- Q: Can two domains take writes to the same key at once? → A: No for the first binary. One source domain A; B is an async log-follower. Writes on B forward to A. `multi_active=on` create is refused until a non-HLC merge exists.
- Q: What does `LOCAL_ONE` write mean on a follower? → A: Still one durable WAL in the **source**. `LOCAL_` does not mean a local WAL on a follower. `LOCAL_*` **read** on a follower MAY be stale local apply.

### Session 2026-09-18

- Q: How does an operator put a node into a quorum domain (the named set of nodes that share one clock and one write-quorum)? → A: Each member belongs to **exactly one** named `quorum_domain` (first-class cluster object, not a ladder key). Bootstrap creates a default domain and places the first node in it. Join must name an existing domain. A node cannot sit in two domains.
- Q: After a peer goes silent on the cluster channel, how should the cluster decide that the peer no longer counts toward write quorum and may be replaced? → A: Fixed **cluster-wide** timeout after the last successful heartbeat (documented default on the order of tens of seconds, configurable). Same event: unavailable for quorum and replace-eligible (`11`).
- Q: When may an administrator promote a follower domain to source if the old source is still up, or if the follower has not applied the whole source log? → A: Ordinary promote is refused unless the follower has applied through the last known source position **or** the old source is heartbeat-timeout unavailable. Force-promote requires an explicit data-loss accept. The old epoch is always fenced.
- Q: If a write arrives in a follower domain and the source domain cannot meet write quorum, what should happen to that request? → A: Fail the write; no local WAL on the follower. Session/query MAY declare a fallback to `LOCAL_ONE` (still one durable WAL in the **source**, not on the follower). If even that cannot be met, the write fails until the source recovers or the follower is promoted.
- Q: May a live member change which quorum domain it belongs to without being decommissioned and joined again? → A: No live change. Decommission or replace, then join naming the new domain.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Always-on cluster ports (Priority: P1)

Every node declares `internode` and `replication` entrypoints. They listen even on a single node. Default bind is loopback; a cluster address is required to join remotes. Transport is `tls` or `plaintext;`. Peers authenticate as the replication role. Unauthenticated peers are refused. Join secret is required before the cluster protocol is spoken (`11`).

**Why this priority**: Placement, Raft, and shuffle have no channel without this.

**Independent Test**: Start one node; confirm both listeners on loopback; omit transport and confirm startup error; connect without replication role and confirm refuse.

**Acceptance Scenarios**:

1. **Given** a single node with no peers, **When** it is `ready`, **Then** `internode` and `replication` accept connections on their declared addresses (default loopback).
2. **Given** omitted `tls`/`plaintext` on either, **When** the node starts, **Then** startup fails (`01`/`14`).
3. **Given** a peer without the replication role or join secret, **When** it connects, **Then** it is refused.
4. **Given** a peer whose internode version is outside N/N+1 (`15`), **When** it connects, **Then** it is refused.
5. **Given** a receiver whose buffers are full, **When** a stream continues, **Then** the sender slows or fails that stream and MUST NOT block worker threads.

---

### User Story 2 - Source domain vs log-follower (Priority: P1)

An operator declares a `quorum_domain` as a first-class cluster object. Each member belongs to **exactly one** named domain (not a topology-ladder key). Bootstrap creates a default domain and places the first node in it; a later join must name an existing domain (`11`). Additional domains are created (`CLUSTER_ADMIN`) before a join can name them. A live member MUST NOT change domain; move is decommission or replace, then join naming the new domain. A container has one source domain A. Replicas in B are async log-followers and never count toward write `ONE`/`LOCAL_ONE`/`TWO`/`QUORUM`. Writes in A are leaderless (or leader-for-ordered types). Writes in B forward to A. If A cannot meet the requested write level, the write fails; B MUST NOT take a local WAL. A session or query MAY declare a fallback to `LOCAL_ONE`, which still requires one durable WAL in A. Promote of B is manual `CLUSTER_ADMIN` with a new epoch; old A is fenced until it rejoins as follower. Ordinary promote is refused unless B has applied through the last known source position **or** A is heartbeat-timeout unavailable. Force-promote requires an explicit data-loss accept.

**Why this priority**: Grill Q19 locked this as the replication contract; `04` placement is incomplete without it.

**Independent Test**: Bootstrap one node (default domain); join a second into that domain and a third into a newly created domain; two domains, RF as in `04` Story 7; write in A and in B; cut A; attempt ordinary promote vs force-promote.

**Acceptance Scenarios**:

1. **Given** bootstrap of the first node, **When** it becomes `ready`, **Then** a default `quorum_domain` exists as a cluster object and that node is its only member. **Given** a join that omits a domain name or names one that does not exist, **When** it is submitted, **Then** it is refused (`11`). **Given** a member of domain A, **When** it is asked to belong also to domain B, **Then** that is refused (exactly one domain per member). **Given** a live member of domain A, **When** `CLUSTER_ADMIN` requests a live domain change to B, **Then** it is refused; the member MUST be decommissioned or replaced and a join MUST name B. **Given** every member labelled `region=eu` and no `quorum_domain` named `eu`, **When** placement or quorum is computed, **Then** `region` is not a domain.
2. **Given** source A and follower B, **When** a write is received in A, **Then** it is handled in A and B applies the source log; B MUST NOT open a source log for that data.
3. **Given** a write received in B, **When** it is submitted, **Then** it is forwarded to A; `LOCAL_ONE` write still requires one durable WAL in A.
3a. **Given** a write received in B whose requested level cannot be met in A, **When** the session/query has no fallback, **Then** the write fails; B MUST NOT acknowledge from a local WAL. **Given** the same write with session/query fallback to `LOCAL_ONE`, **When** one durable WAL in A can still be obtained, **Then** the write succeeds at `LOCAL_ONE` and the response names the level that was met. **Given** fallback to `LOCAL_ONE` but no durable WAL in A (source partitioned), **When** the write is submitted, **Then** it still fails; B MUST NOT accept it locally. Clients in B cannot write until A recovers or B is promoted.
4. **Given** write `TWO`/`QUORUM`, **When** only B replicas acknowledge, **Then** the write is not successful; only durable A replicas count.
5. **Given** `EACH_QUORUM` without wait-for-apply, **When** it is requested, **Then** it is refused (`04`); with wait, ack requires A quorum plus B apply of that log position.
6. **Given** `LOCAL_ONE` read in B, **When** the local replica has applied, **Then** it MAY return stale data; stronger reads forward to A if B cannot satisfy them.
7. **Given** ordered/log types, **When** an independent write is attempted in B, **Then** it is forwarded to the leader in A or refused.
8. **Given** B has applied through the last known source position, **When** `CLUSTER_ADMIN` requests ordinary promote of B, **Then** it succeeds: B is source at a new epoch and old A is fenced (even if A is still heartbeating) until it rejoins as follower. Two live sources for one container are refused.
8a. **Given** A is still heartbeating and B has not applied through the last known source position, **When** ordinary promote is requested, **Then** it is refused. **Given** the same state, **When** force-promote is requested **without** an explicit data-loss accept, **Then** it is refused. **Given** an explicit data-loss accept, **When** force-promote is requested, **Then** it succeeds, the old epoch is fenced, and the accept is recorded.
8b. **Given** every member of A is heartbeat-timeout unavailable, **When** ordinary promote of B is requested, **Then** it is allowed even if catch-up against a live A head cannot be proven. The old epoch is fenced. A later return of old A MUST NOT accept writes until it rejoins as follower.
9. **Given** `multi_active=on` on create, **When** submitted in the first binary, **Then** it is refused. Catalog default is off; ordered/log types forced off.

---

### User Story 3 - HLC in-domain, source log across domains (Priority: P1)

Inside one `quorum_domain`, leaderless writes stamp HLC; LWW by HLC is the default; type-supported merge is optional. HLC MUST NOT be compared across domains. Cross-domain copy follows source log sequence. Clock skew inside a domain is measured; beyond tolerance is a health problem, not silent reordering.

**Why this priority**: Conflict resolution was unspecified; grill closed LWW vs human-pick.

**Independent Test**: Concurrent writes in A; concurrent apply on B; inject skew past tolerance.

**Acceptance Scenarios**:

1. **Given** two replicas in A with different values, **When** they reconcile, **Then** they converge on the later HLC stamp (or the type's declared merge); disagreement is recorded; stale replicas are corrected.
2. **Given** values in A and B, **When** they disagree, **Then** B applies the source log position; local HLC on B MUST NOT win.
3. **Given** skew inside A beyond documented tolerance, **When** it is observed, **Then** `node_state` (or equivalent) is degraded; writes are not silently reordered without that signal.
4. **Given** types that need total order, **When** they write, **Then** they use control-plane leadership / Raft log (`06`), not LWW.

---

### User Story 4 - Failure detection and partitions (Priority: P2)

Peers are marked unavailable by a **fixed cluster-wide** heartbeat timeout on `internode` (crash-stop, not Byzantine). The documented default is on the order of tens of seconds and is configurable. That same event makes the peer unavailable for quorum counting **and** replace-eligible (`11`). Data path is tunable-AP; controller groups are CP (minority MUST NOT mutate membership/schema). Repair/anti-entropy, read repair, and hinted handoff exist so quorum clusters do not drift; `04` owns placement repair, this feature owns the streams.

**Why this priority**: Quorum counting without failure detection lies.

**Independent Test**: Stop a peer; after the cluster-wide timeout it does not count and replace is allowed; restore heartbeat and confirm it counts again; confirm repair stream.

**Acceptance Scenarios**:

1. **Given** a silent peer, **When** the cluster-wide heartbeat timeout elapses, **Then** every remaining member treats it as unavailable for quorum counting and replace-eligible (`11`) until it heartbeats again or is decommissioned. **Given** the peer heartbeats again before that timeout, **When** a write quorum is computed, **Then** it still counts. **Given** two observers, **When** the timeout elapses, **Then** they agree (same cluster-wide value; not a per-node guess).
2. **Given** a minority controller partition, **When** membership/schema change is attempted there, **Then** it is refused (`06`).
3. **Given** data split inside a domain, **When** it heals, **Then** LWW/HLC (or type merge) converges; two source domains after botched promote remain refused (epoch fence).
4. **Given** anti-entropy, **When** replicas drift, **Then** repair streams run on `internode`/`replication` and are throttleable (`04`).

---

### Edge Cases

- Measured RTT MUST be published so the planner (`05`) can override ladder rank (`04`).
- Two bootstraps with the same name are two clusters (`11`), not this feature.
- Buffer-full backpressure MUST NOT block the worker pool.
- Promote without fence: protocol-refused; old epoch writes MUST NOT be accepted.
- Ordinary promote while the old source is still heartbeating and the follower is not caught up: refused.
- Force-promote without an explicit data-loss accept: refused.
- Join that omits `quorum_domain` or names an unknown domain: refused (`11`).
- A node MUST NOT be a member of two `quorum_domain`s at once.
- Live change of a member's `quorum_domain`: refused. Move is decommission or replace, then join naming the new domain (`11`).
- Topology labels (`region`, `planet`, …) NEVER become a domain, even when all members share a value.
- Failure-detector timeout is cluster-wide: observers MUST NOT use different local values for the same peer. A still-heartbeating peer MUST still count toward quorum and MUST NOT be replace-eligible.
- Write received in a follower while the source cannot meet the requested level: fail; no local WAL on the follower. Session/query fallback to `LOCAL_ONE` still requires one durable WAL in the source; it MUST NOT ack from the follower. Silent fallback without that declaration MUST NOT occur.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Cluster-internal traffic MUST use `internode` and `replication` handlers, not client protocol ports, and MUST NOT share a port with `admin`/`admin-http`.
- **FR-002**: Both handlers MUST listen even on a single node. Default bind loopback; cluster address required to join remotes (`11`).
- **FR-003**: Transport MUST be `tls { ... }` or `plaintext;`. Peers MUST authenticate as the replication role. Join secret required before cluster protocol (`11`).
- **FR-004**: Internode protocol MUST be versioned; refuse peers outside N/N+1 (`15`).
- **FR-005**: A `quorum_domain` MUST be an explicit cluster object (HLC + synchronous write-quorum set), not a topology-ladder key. Each member MUST belong to exactly one named domain. Bootstrap MUST create a default domain and place the first node in it. Join MUST name an existing domain (`11`); omit or unknown name MUST be refused. A node MUST NOT belong to two domains. A live member MUST NOT change domain; the only move is decommission or replace, then join naming the new domain (`11`). Additional domains MUST be created as cluster objects (`CLUSTER_ADMIN`) before a join can name them. Label keys MUST NOT silently become a domain. `planet` MUST NEVER be a domain.
- **FR-006**: Each container MUST have one source domain A; other domains are async log-followers and MUST NOT count toward write `ONE`/`LOCAL_ONE`/`TWO`/`QUORUM`.
- **FR-007**: Writes in A are leaderless (or leader-for-ordered). Writes in B MUST forward to A. B MUST NOT open a source log for replicated data and MUST NOT acknowledge a write from a local WAL. Ordered/log types MUST NEVER accept independent writes in B. If A cannot meet the requested write level, the write MUST fail unless a session/query fallback applies (FR-008). The cluster MUST NOT auto-promote to accept the write.
- **FR-008**: `LOCAL_ONE` write = one durable WAL in the **source** domain, including when the coordinator is in a follower domain. `LOCAL_*` read on B MAY be stale local apply. Stronger reads MUST forward to A when B cannot satisfy the level. A session or query MAY declare a fallback from a stronger write level to `LOCAL_ONE` (`04`/`02` carry the option). The fallback MUST still require one durable WAL in the source; the response MUST name the level that was met. Fallback MUST NOT mean a local WAL on a follower. If even one durable source WAL cannot be obtained, the write MUST fail. Silent (undeclared) fallback MUST NOT occur.
- **FR-009**: `EACH_QUORUM` = source quorum plus follower apply of that log position, or refused without wait-for-apply (`04`).
- **FR-010**: Promote MUST be manual `CLUSTER_ADMIN` and MUST create a new epoch and fence the old source until it rejoins as follower. Two live sources MUST be refused. Ordinary promote MUST be refused unless the follower has applied through the last known source position **or** the old source is heartbeat-timeout unavailable. Force-promote MUST require an explicit data-loss accept and MUST still fence the old epoch. Force-promote without that accept MUST be refused.
- **FR-011**: `multi_active` catalog flag default off; ordered/log forced off; create-on refused in the first binary (`16`).
- **FR-012**: HLC is the default version stamp **inside** a domain only. MUST NOT compare HLC across domains. Cross-domain apply is source log sequence.
- **FR-013**: In-domain conflict default is LWW by HLC; type-supported deterministic merge optional. Concurrent writes MUST NOT wait for a human.
- **FR-014**: Skew inside a domain beyond tolerance MUST be a cluster health problem.
- **FR-015**: Failure detection is a **fixed cluster-wide** heartbeat timeout on `internode` (crash-stop, not Byzantine). The documented default is on the order of tens of seconds and MUST be configurable. After the last successful heartbeat plus that timeout, the peer MUST be unavailable for quorum counting and MUST be replace-eligible (`11`) until it heartbeats again or is decommissioned. Observers MUST share the same timeout value. Phi-accrual and per-observer timeouts MUST NOT be used.
- **FR-016**: Data path is tunable-AP; controllers are CP (`06`).
- **FR-017**: Repair/anti-entropy, read repair, and hinted handoff MUST exist as internode streams; placement repair policy is `04`.
- **FR-018**: Internode RTT and HLC skew MUST be measured for planner override of ladder rank (`05`).
- **FR-019**: Backpressure MUST slow or fail the specific stream, not block worker threads.
- **FR-020**: Durable vs memory meaning of an acknowledgement is `13`; this feature owns which nodes count.

### Key Entities

- **Internode / Replication Entrypoints**: Always-on cluster ports.
- **Quorum Domain**: First-class named cluster object: one HLC and one synchronous write-quorum set. Each member belongs to exactly one. Bootstrap creates a default; join names an existing one. Not a ladder key. No live reassignment; move is decommission or replace then join.
- **Source Domain / Log-Follower**: Per-container roles.
- **Write-level fallback**: Session/query option to meet `LOCAL_ONE` (source WAL) when a stronger requested write level cannot be met. Not a follower-local ack.
- **HLC Stamp**: In-domain version; not comparable across domains.
- **Source Log Position**: Cross-domain apply order.
- **Epoch / Fence**: Promote safety. Ordinary promote (caught up, or old source unavailable) vs force-promote (explicit data-loss accept). Old epoch writes refused.
- **Failure Detector**: Cluster-wide heartbeat timeout on `internode`. Same event for quorum unavailability and replace eligibility (`11`).
- **`multi_active`**: Catalog flag; create-on refused in first binary.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of single-node starts have `internode` and `replication` listening; 100% of omitted-transport starts fail.
- **SC-002**: 100% of writes in the suite that are acknowledged at `TWO`/`QUORUM` counted only durable source-domain replicas.
- **SC-003**: 100% of writes received in a follower domain are forwarded; 0 independent follower source logs for replicated data; 100% of writes that cannot meet the requested source level fail unless session/query fallback to `LOCAL_ONE` is declared; fallback still requires a durable source WAL in 100% of tests.
- **SC-004**: 100% of `LOCAL_ONE` writes on a follower require a durable source WAL; 100% of `LOCAL_ONE` reads on a follower MAY be served locally.
- **SC-005**: 100% of `multi_active=on` creates in the first-binary suite are refused.
- **SC-006**: 100% of in-domain conflicts in the suite converge by HLC LWW (or declared merge); 100% of cross-domain copies follow source log order.
- **SC-007**: Ordinary promote succeeds in 100% of caught-up tests and in 100% of old-source-unavailable tests; ordinary promote of a lagging follower while the old source still heartbeats fails in 100% of tests; force-promote without data-loss accept fails in 100% of tests; every successful promote (ordinary or force) creates a new epoch and fences the old source; two live sources occur in 0 tests.
- **SC-008**: 100% of bootstrap tests create a default `quorum_domain` and place the first node in it; 100% of joins that omit or name an unknown domain are refused; 0 tests leave a member in two domains; 100% of live domain-change attempts are refused.
- **SC-009**: After the cluster-wide heartbeat timeout, 100% of tests treat the silent peer as unavailable for quorum and replace-eligible; 0 tests count a timed-out peer or replace a still-heartbeating peer.

## Assumptions

- Replica targets (who may hold a copy) are `04`; this feature owns who counts for a given level. Session/query fallback to `LOCAL_ONE` is carried by `04`/`02`; this feature owns that the fallback still counts a source-domain WAL.
- Durable ack = WAL fsync for persistent/hybrid (`13`).
- Membership/join secret (`11`); Raft on `internode` (`06`); shuffle client (`05`). Join (`11`) enforces that the named `quorum_domain` exists; this feature owns the object and the one-domain-per-member rule. Moving a node between domains is decommission or replace plus join (`11`), not a live edit. Replace eligibility consumes this feature's heartbeat-timeout event; exact default seconds are documented with starter configs.
- Threat model: hostile tenants, operator-run nodes, crash-stop (`16`).

## Out of Scope

- Membership bootstrap/join/leave (`11`).
- Placement RF, labels, anti-affinity (`04`).
- What an acknowledgement means on disk (`13`).
- Client protocols (`02`).
- Key material (`14`).

- **Human-picked multi-master conflict** — product non-goal (`16`); concurrent writes MUST NOT wait for a human (LWW by HLC or type merge only).
- **Byzantine / adversarial nodes** — product non-goal (`16`); failure detection is crash-stop.
- **`multi_active=on` create in the first binary** — refused (`16`); catalog default off.
