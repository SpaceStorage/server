# Feature Specification: Cluster Identity, Discovery, Join, Leave, and Replace

**Feature Branch**: `011-identity-membership`

**Created**: 2026-09-15

**Status**: Draft

**Input**: User description: "Read .specify/intent/11-identity-membership.md and specify this feature." — cluster name (label) plus UUID identity plus join secret; stable node identity independent of hostname; seed discovery; bootstrap vs join; join = secret + CLUSTER_ADMIN admit or one-time token; drain / decommission / replace; membership is input to Raft (`06`); ladder labels required on join (`04`).

## Clarifications

### Session 2026-09-15

- Q: Is cluster name unique identity? → A: No. Name is a human label. Identity is a random UUID generated at bootstrap plus a join secret. Same name on two bootstraps is two clusters; there is no merge protocol.
- Q: Does possessing the join secret make a node a replica target? → A: No. The secret is necessary to speak `internode`/`replication`. Membership still requires `CLUSTER_ADMIN` admit or a one-time join token (bound to node name, single use, TTL on the order of hours, audit-logged). Stolen secret without admit/token MUST NOT add a member. Secret is rotatable with an overlap window.

### Session 2026-09-18

- Q: When a node that is already in the cluster membership view restarts, what must it show before it may become `ready` again? → A: Persisted membership + join secret only. No new admit/token. Seeds are for first join; the node finds peers from the stored membership view. If no peer is reachable, it MAY still become `ready` from local state (full-cluster and last-node restart work). FR-008 applies only to processes not yet in membership. Membership stays mutable: `CLUSTER_ADMIN` MAY decommission (remove) members that will not return and join new members; surviving members MUST NOT be required to re-admit.
- Q: When may an operator reuse a member's node identity with replace, instead of decommissioning that member and joining a new node? → A: After the failure-detector timeout (`12`) the identity is eligible for replace with no extra liveness check and no separate mark-dead step. A member that is still heartbeating (`ready` or `draining`) MUST refuse replace. Replace still requires authorization (`CLUSTER_ADMIN` or documented procedure). After replace commits, the previous incarnation of that identity MUST be fenced (refused if it later speaks). Decommission + join (new identity) remains available.
- Q: When an operator decommissions a node that is still running, must that process stay up so replicas can be copied off? → A: Operator drain is a cluster state: the process stays up (no new tenant connections, no new placements) until decommission finishes, the operator undrains (back to `ready`), or a stop signal is sent. A stop signal drains then exits (`01`) and is what rolling restart uses. Live decommission copies replicas off the still-running draining node.
- Q: After a node is decommissioned and removed from membership, may a later join reuse that node's name or identity? → A: After decommission the **name** MAY be reused (unique among **current** members only). The **identity** is retired; a later join MUST use a new identity. Identity reuse is **replace** only, and only for an identity that is still in membership and replace-eligible (failure-detector timeout). A first join that presents a retired identity MUST be refused.
- Q: Can a new node sit as a pending joiner until an administrator admits it, or must every first join present a pre-issued one-time token? → A: Both. After the join secret (and labels) are accepted, the process MAY wait as a **pending join** (not a member: no vote, not a replica target, MUST NOT be `ready` as a member) until `CLUSTER_ADMIN` admits it. A one-time token bound to name is the unattended path and MUST NOT wait for a second admit. Stolen secret still MUST NOT add a member.
- Q: After decommission, may the cluster have zero remaining members, or must at least one member stay? → A: Refuse decommission of the last remaining member. The cluster always has ≥1 member. Destroying a cluster is stop + wipe data dirs, not decommission. Data-loss accept MUST NOT bypass this.
- Q: If a pending join process has already exited, may an administrator still admit that identity and record it as a member? → A: Refuse admit unless the pending process is still connected. Exit drops the pending row; a later start of the same identity is a new first join (pending or token).
- Q: When replacing a failed member, how must the new process present that member's node identity? → A: Operator presents the existing node identity; a fresh data dir is allowed. Copying the old identity files is one way, not required. Authorization plus failure-detector timeout fence the old incarnation.
- Q: When an administrator mints a one-time join token, what must that token be bound to? → A: Node name only for the token's life. Identity is not part of the bind.
- Q: If a process already has a persisted cluster identity on disk, what must happen when its config still declares bootstrap? → A: If local cluster identity exists, ignore the bootstrap declaration and start as that cluster (member restart). Bootstrap creates UUID only when no local cluster identity exists. Wipe the data dir to form a new cluster on that disk.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Bootstrap the first node (Priority: P1)

An operator starts the first node with an explicit bootstrap declaration and an empty or self-only seed list. The node creates the cluster UUID and join secret, becomes the initial cluster-level controller, and reaches `ready` as the sole member. A second process that bootstraps with the same human name on a **different empty disk** creates a **different** cluster. Restart of the first node with bootstrap still declared MUST keep the same UUID.

**Why this priority**: Nothing in membership, Raft, or placement exists without the first identity.

**Independent Test**: Bootstrap one node; record UUID and secret; restart it with bootstrap still declared and confirm the same UUID; attempt a second bootstrap with the same name on an isolated empty disk; confirm two identities.

**Acceptance Scenarios**:

1. **Given** a node with an explicit bootstrap declaration, no foreign seeds, and **no** local cluster identity, **When** it starts, **Then** it creates a cluster UUID and join secret, is the sole member, and reaches `ready`.
2. **Given** two isolated bootstraps with the same cluster name on empty disks, **When** both complete, **Then** they have different UUIDs and MUST NOT merge if later networked.
3. **Given** a process that is **not** in the membership view, is not bootstrapping, and cannot reach any seed, **When** it starts, **Then** it MUST NOT become `ready` as a member.
4. **Given** a process that already has a local cluster identity, **When** it starts with bootstrap still declared, **Then** it MUST NOT create a new UUID; it starts as that cluster (member restart). Forming a new cluster on that disk requires wipe of the data dir.

---

### User Story 2 - Join a second node (Priority: P1)

An operator starts a subsequent node with the join secret, its node identity, a unique node name, and every key on the cluster topology ladder (`04`). The process MAY wait as a **pending join** until `CLUSTER_ADMIN` admits it, or it MAY present a valid one-time token and skip that wait. Only after admit or token is membership recorded cluster-wide. Joining does not by itself move existing replicas.

**Why this priority**: Three-node quorum in the first binary (`16`) depends on join.

**Independent Test**: Bootstrap A; start B with secret only and confirm pending (not a member); admit B; omit a ladder key and confirm refuse; use stolen secret without admit/token and confirm not a replica target; join C with a one-time token and confirm no second admit. Stop a pending process and confirm admit is refused and a later start is a new first join.

**Acceptance Scenarios**:

1. **Given** member A and a join secret, **When** B presents secret, identity, unique name, and complete ladder labels **without** a token, **Then** B is a **pending join** (not in membership, MUST NOT vote, MUST NOT be a replica target, MUST NOT be `ready` as a member) until `CLUSTER_ADMIN` admits it; **When** admit is recorded, **Then** every live member converges on the same membership view including B, and B MAY receive client requests.
2. **Given** B omits a ladder key or fails hierarchy integrity (`04`), **When** it joins, **Then** join is refused (no pending join is created).
3. **Given** only the join secret and no admit/token, **When** a process speaks `internode`, **Then** it MUST NOT become a replica target or vote (pending is not membership).
4. **Given** a one-time token already used or expired, **When** it is presented, **Then** join is refused and the attempt is audit-logged (`14`).
5. **Given** a join, **When** existing containers are described, **Then** their replica placements are unchanged until rebalancing (`04`).
6. **Given** a decommissioned member's **name** and a **new** node identity, **When** a first join presents that name plus secret and admit/token, **Then** join is accepted (name unique among current members). **Given** a first join that presents a **retired** node identity, **When** it is not an authorized replace of a still-membered identity, **Then** join is refused.
7. **Given** a valid unused one-time token bound to B's **name**, **When** B presents secret, identity, labels, and that token, **Then** membership is recorded without a second `CLUSTER_ADMIN` admit and every live member converges on B. **Given** a token bound to name B, **When** a process presents that token with a **different** name, **Then** join is refused. A different node identity with the same name MUST be accepted if the name is unique among current members and the token is unused.
8. **Given** a pending join whose process has exited, **When** `CLUSTER_ADMIN` admits that identity, **Then** admit is refused, the pending row is gone, and no membership is recorded. **Given** a later start of that same identity, **When** it presents secret and labels again, **Then** it is a new first join (pending or token).

---

### User Story 3 - Drain, decommission, and replace (Priority: P1)

An operator drains a node (no new tenant connections, no new replica placements; the process stays up), decommissions it (re-place replicas from that still-running node then remove membership; the last remaining member cannot be decommissioned), or replaces a member that the failure detector has marked unavailable (timeout, `12`) by presenting that member's node identity on a new process (a fresh data directory is allowed). A still-heartbeating node refuses replace; there is no extra liveness check. Rolling restart: stop-signal drain (process exits), restart, wait until `ready` and replica catch-up, then the next. Restart of an already-admitted member does not consume a new admit or token. After restart, the operator may still remove members that will not return (except the last remaining member) and join new ones.

**Why this priority**: Operators cannot add or remove machines without these procedures.

**Independent Test**: Three-node RF=2; operator-drain one (process stays up) and decommission it onto remaining capacity; replace a killed node after failure-detector timeout using a new process that presents the same identity on a fresh data dir. Restart all three without a new admit; decommission one that will not return and join a fourth with admit/token. Rolling restart uses stop-signal drain then start. Decommission of the sole remaining member is refused.

**Acceptance Scenarios**:

1. **Given** a `ready` member, **When** the operator marks it `draining`, **Then** the process stays up, it accepts no new tenant-protocol connections, is not chosen for new replica placements, and finishes in-flight tenant work (timeout for those requests is `01`). Existing replicas remain until rebalance or decommission. **Given** undrain, **Then** it returns to `ready` and MAY accept new tenant connections and new placements. **Given** a stop signal (`01`), **Then** it drains and the process exits.
2. **Given** decommission of a still-running draining member, **When** it completes, **Then** replicas have been copied/re-placed from that node onto members that satisfy container constraints (`04`), or the operator explicitly accepted data loss for unreplicated containers, and the node is removed from membership.
3. **Given** decommission that cannot satisfy some container's constraints, **When** it runs, **Then** it stops short and names the blocking containers (`04`); the draining process is still up.
4. **Given** a member whose failure-detector timeout has elapsed with no heartbeat (`12`), **When** a new process is authorized to replace that identity (presenting the existing node identity; a fresh data directory is allowed), **Then** replace is allowed with no extra liveness check, placements that named that identity need not all be rewritten, and the previous incarnation is fenced. **Given** a member that is still heartbeating, **When** replace is requested, **Then** it is refused. Copying the old identity files is sufficient but MUST NOT be required.
5. **Given** rolling restart of three members, **When** each is stop-signal drained, restarted, and caught up in turn, **Then** the cluster remains available at the containers' declared quorum levels, and no restart required a new admit or join token.
6. **Given** a process already recorded in membership, **When** it restarts with the join secret and no new admit/token, **Then** it becomes `ready` as that member. **Given** no peer or seed is reachable, **Then** it MAY still become `ready` from local membership (last-node and full-cluster restart).
7. **Given** surviving members after a restart, **When** `CLUSTER_ADMIN` decommissions a member that will not return and joins a new node with a **new** identity, secret plus admit or token, **Then** the new node is added, the removed node is gone from membership, surviving members MUST NOT be required to re-admit, and the decommissioned **name** MAY be reused on that new join.
8. **Given** a cluster with exactly one remaining member, **When** decommission of that member is requested (including with data-loss accept), **Then** decommission is refused and that member stays in membership. Retiring the cluster is stop and wipe of data dirs, not this procedure.

---

### Edge Cases

- Node name collision inside the cluster: refuse join naming a **current** member. After decommission, that name MAY be used again.
- Address/hostname change of an existing identity: accepted; identity MUST NOT change.
- Join secret rotation: overlap window where old and new both work, then old dies.
- Non-member MUST NOT vote (`06`) and MUST NOT be a new replica target.
- Mixed-version join: N/N+1 window is `15`; this feature only presents identity.
- Process not in membership, not bootstrapping, no reachable seed: MUST NOT become `ready` as a member.
- Already-admitted member restart with no reachable seed: MAY become `ready` from local membership; MUST NOT require a new admit/token.
- Replace eligibility: failure-detector timeout with no heartbeat (`12`) is sufficient; no extra liveness check. Still-heartbeating `ready`/`draining` MUST refuse replace.
- After replace commits: previous incarnation of that identity MUST be refused if it speaks again. The replacement process presents the existing node identity; it MUST NOT be required to start from the failed member's on-disk identity directory.
- Operator drain while decommission is in progress: process stays up so replicas can be copied off; a stop signal during that drain exits the process (`01`) and remaining copy MUST then use other replicas or the operator's data-loss accept.
- First join presenting a retired (decommissioned) node identity: refused. Identity reuse is replace of a still-membered, timeout-unavailable identity only.
- Pending join that exits before admit: the pending row is dropped; admit MUST be refused while that process is not connected; no membership is recorded; a later start of the same new identity is again a first join (pending or token).
- Pending join MUST NOT be listed as a member in the membership view.
- One-time join token is bound to **node name only**; a different identity with that name is accepted if the name is unique among current members and the token is unused. A mismatched name MUST refuse.
- Decommission of the last remaining member: refused, including with data-loss accept. Cluster always has ≥1 member. Cluster destroy (stop + wipe data dirs) is not decommission.
- Bootstrap declared but local cluster identity already exists: ignore bootstrap; start as that cluster; do not create a new UUID. New cluster on that disk requires wipe.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Every cluster MUST have a human **name** (not unique in the universe), a **UUID identity** generated at bootstrap that does not change for the life of that cluster, and a **join secret**.
- **FR-002**: Same human name on two bootstraps MUST be two clusters. There MUST NOT be a protocol that detects or merges them across isolated networks.
- **FR-003**: Every node MUST have a stable **node identity** persisted on first start, independent of hostname, IP, and PID, and a **node name** unique among **current** members. After decommission, that name MAY be reused. A node identity MUST NOT be reused by a first join once it has been decommissioned (retired). For **replace**, the operator MAY present an existing still-membered identity on a new process, including with a fresh data directory.
- **FR-004**: Discovery of a cluster for a **first join** MUST be a documented seed list of already-member nodes (names or addresses). An already-admitted member MUST discover peers from the stored membership view; the seed list MUST NOT be a restart gate for members.
- **FR-005**: Bootstrap MUST be explicit: first node, empty or self-only seed list, **and no local cluster identity**, creates UUID + join secret, becomes initial cluster-level controller. If a local cluster identity already exists, the process MUST ignore the bootstrap declaration and start as that cluster (FR-016). Bootstrap MUST NOT create a new UUID on a disk that already has one. Forming a new cluster on that disk requires wipe of the data dir.
- **FR-006**: A **first join** (process not yet in membership) MUST present the join secret, node identity, node name, and every topology-ladder key (`04`). Secret is necessary to open cluster ports. After those are accepted, the process MAY wait as a **pending join** until `CLUSTER_ADMIN` admits it, or it MAY present a valid unused one-time join token (bound to **node name only**, single use, hours-scale TTL, audit-logged) and MUST become a member without a second admit. Membership MUST NOT be recorded until admit or token. Admit MUST be refused unless that pending process is still connected. If the pending process exits, the pending row MUST be dropped and a later start of that identity MUST be a new first join. Restart of an already-admitted member MUST NOT consume a new admit or token.
- **FR-007**: Stolen secret without admit/token MUST NOT add a member. A pending join MUST NOT vote, MUST NOT be a replica target, and MUST NOT be `ready` as a member. The join secret MUST be rotatable with an overlap window.
- **FR-008**: A process that is **not** in the membership view, is not bootstrapping, and cannot reach any seed MUST refuse to become `ready` as a member. This MUST NOT apply to an already-admitted member.
- **FR-009**: Every live member MUST converge on the same membership view. Non-members (including pending joins) MUST NOT vote and MUST NOT be replica targets for new placements.
- **FR-010**: Operator **drain** MUST mark the member `draining`, MUST stop new tenant connections and new replica placements, and MUST keep the process running. Existing replicas remain until rebalancing or decommission. Drain ends when decommission removes membership, when the operator **undrains** (member returns to `ready` and MAY take new tenant connections and new placements), or when a **stop signal** is received. A stop signal MUST drain then exit the process (`01`). Rolling restart MUST use the stop-signal path.
- **FR-011**: Decommission of a still-running draining member MUST copy/re-place replicas from that node onto members that satisfy container constraints (`04`) before removing membership, unless the operator explicitly accepts data loss for unreplicated containers. If the process is already gone, decommission MUST re-place only from remaining replicas or require that data-loss accept. Decommission MUST be refused when the target is the last remaining member; data-loss accept MUST NOT bypass this.
- **FR-012**: A member MUST be **replace-eligible** when the failure detector (`12`) has marked it unavailable (heartbeat timeout elapsed). No extra liveness check and no separate mark-dead step are required. A member that is still heartbeating (`ready` or `draining`) MUST refuse replace. Replace MUST present that existing node identity (a fresh data directory is allowed; copying the failed member's identity files is one way, not required) so placements that named it need not all be rewritten. Replace MUST prove authorization (`CLUSTER_ADMIN` or documented procedure). After replace is recorded in membership (`06`), the previous incarnation of that identity MUST be fenced: it MUST NOT vote, MUST NOT be a replica target, and MUST be refused on `internode`/`replication`.
- **FR-013**: Membership changes MUST be recorded in cluster-level controller storage (`06`) before the node votes or becomes a replica target. Recording MUST happen on admit or token, not when a pending join is created.
- **FR-014**: Every **member** node MAY receive client requests, including a node that holds no replica of the target container.
- **FR-015**: Rolling restart MUST be stop-signal drain one node at a time (process exits); restart; wait until `ready` and replica catch-up before draining the next. Each restarted member MUST use FR-016 (no new admit/token).
- **FR-016**: An already-admitted member that restarts MUST become eligible for `ready` with persisted membership plus the join secret only. If no peer is reachable, it MAY still become `ready` from local membership (last-node and full-cluster restart). A leftover bootstrap declaration MUST NOT change this (FR-005).
- **FR-017**: Membership MUST remain mutable after bootstrap and after restarts: `CLUSTER_ADMIN` MAY decommission (remove) members other than the last remaining member and MAY join new members. Surviving members MUST NOT be required to re-admit when others are removed or added.
- **FR-019**: A cluster MUST always have at least one member. Destroying a cluster is stop of processes and wipe of data dirs, not a membership procedure.
- **FR-018**: After decommission, a later first join MAY reuse the decommissioned **node name** and MUST present a **new node identity**. Identity reuse MUST be the replace procedure (FR-012) only. A first join that presents a retired identity MUST be refused.
- **FR-020**: A bootstrap declaration is honored only when the process has no local cluster identity. Presence of local cluster identity MUST cause member restart of that cluster, not a second bootstrap.

### Key Entities

- **Cluster Identity**: UUID + join secret; name is a label only. Created once per disk at bootstrap; a later bootstrap declaration on the same disk does not replace it.
- **Node Identity**: Stable unique id persisted on first start. Retired on decommission; reuse only via replace of a still-membered identity. Replace MAY present that id on a fresh data directory.
- **Node Name**: Human label unique among current members; reusable after decommission.
- **Seed List**: Discovery of members.
- **Join Token**: One-time, bound to **node name only**, TTL, audit-logged unattended admit; skips pending wait. Node identity is not part of the bind.
- **Pending Join**: Process that presented secret + identity + name + ladder labels and is waiting for `CLUSTER_ADMIN` admit. Not a member. Exists only while that process is connected; exit drops the row; admit of a disconnected pending identity is refused.
- **Membership View**: Identical on every live member; input to Raft and placement. Does not include pending joins.
- **Drain / Decommission / Replace / Undrain**: Operator drain is a still-running `draining` state; stop signal drains then exits (`01`); undrain returns a draining member to `ready`. Decommission is refused for the last remaining member. Replace-eligible means the failure detector has marked the member unavailable (`12`); still-heartbeating members are not eligible.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Bootstrap of a first node on a disk with no local cluster identity creates UUID + secret and a one-member `ready` cluster in 100% of conformance runs. Restart of that node with bootstrap still declared keeps the same UUID in 100% of runs.
- **SC-002**: Two isolated bootstraps with the same name produce different UUIDs in 100% of runs and never merge.
- **SC-003**: 100% of joins missing a ladder key, hierarchy integrity, or secret are refused (no pending join). 100% of secret-accepted first joins without admit and without an unused token remain pending-only. 100% of complete joins (admit or unused token) converge membership on every live member.
- **SC-004**: Secret-only and pending-join processes are replica targets in 0 cases and cast 0 votes.
- **SC-005**: Operator drain keeps the process up and excludes new tenant connections and new placements in 100% of tests; undrain returns the member to `ready` in 100% of tests; stop-signal drain exits the process (`01`) in 100% of tests; live decommission copies/re-places from that still-running node or names blockers; gone-process decommission uses remaining replicas or the data-loss accept.
- **SC-006**: After failure-detector timeout, authorized replace of that identity succeeds in 100% of tests with no extra liveness check, including when the new process uses a fresh data directory and presents the existing identity; replace of a still-heartbeating member fails in 100% of tests; a fenced previous incarnation is refused in 100% of return-after-replace tests.
- **SC-007**: 100% of already-admitted member restarts reach `ready` without a new admit or token, including the case where no seed or peer is reachable and the case where bootstrap remains declared; 0 surviving members are required to re-admit when another member is decommissioned and a new member is joined.
- **SC-008**: After decommission, a first join reusing the retired **name** with a **new** identity succeeds in 100% of tests; a first join presenting the retired **identity** is refused in 100% of tests.
- **SC-009**: 100% of pending joins become members only after `CLUSTER_ADMIN` admit of a still-connected pending process; 100% of admit attempts after that process has exited are refused with no membership recorded; 100% of valid unused tokens bound to the presented **name** record membership without a second admit; 100% of tokens presented with a non-matching name are refused.
- **SC-010**: Decommission of the last remaining member is refused in 100% of tests (including with data-loss accept); after every successful decommission the membership view still has ≥1 member.

## Assumptions

- Internode framing and authentication as the replication role are `12`/`14`; this feature owns identity and membership procedures.
- Topology ladder keys and hierarchy integrity rules are `04`; this feature enforces them at join.
- Raft consumption of membership is `06`.
- Mixed-version rules are `15`.
- Drain timeout and tenant connection close are `01`.
- Heartbeat / timeout failure detection is `12`; this feature consumes “unavailable” for replace eligibility.

## Out of Scope

- Internode wire, clocks, conflict resolution (`12`).
- WAL/fsync (`13`).
- Raft election internals beyond consuming membership (`06`).
- Replica rebalancing algorithms (`04`) except as a consumer of drain/leave.
- AuthN of the operator (`14`) except that these operations require the admin role / `CLUSTER_ADMIN`.
- Wire protocol handlers (`02`).
- Destroying a cluster by stopping processes and wiping data dirs (not a decommission procedure).
