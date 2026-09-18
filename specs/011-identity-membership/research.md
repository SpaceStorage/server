# Research: Cluster Identity, Discovery, Join, Leave, and Replace

**Feature**: `011-identity-membership` | **Date**: 2026-09-18

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-15 and 2026-09-18), constitution 1.3.0, intent `11`, sibling plans `001` (lifecycle/stop), `004` (`ClusterStore`, ladder, `cluster.token_file`), `006` (cluster log, voter set), `012` (internode, heartbeat), `014` (CLUSTER_ADMIN, audit), `016` (first binary).

No `NEEDS CLARIFICATION` remains in Technical Context.

## R1. One crate owns procedures, not the store

- **Decision**: Add `crates/membership` (`spacestorage-membership`). It is the only writer of membership/pending/token/retire/replace **events**. Applied state lives in the cluster `ClusterStore` (`004` seam, `006` Raft impl). Placement reads “member / draining / not a replica target” from that snapshot.
- **Rationale**: `006` already stores `MemberRecord` and forbids non-members from voting. Putting join/secret/drain orchestration inside `controlplane` would mix Raft with operator procedures. Putting it in `placement` would pull secrets into the planner.
- **Alternatives considered**: Grow `controlplane` (rejected: dep and test blast radius); grow `node` only (rejected: conformance and CLI would duplicate rules); standalone membership process (rejected: Principle III).

## R2. `004` `cluster.token_file` is the join secret

- **Decision**: The existing `cluster { token_file PATH; }` directive **is** the join secret file (`012` “secret required before cluster protocol”). Bootstrap **writes** that file (and `{data_dir}/identity/cluster.json`). Join **reads** it. Rotation is a two-phase overlap on the same path plus an epoch list in cluster state. No second `join_secret_file`.
- **Rationale**: `004` already gated internodes on this file. A parallel secret would split auth.
- **Alternatives considered**: Separate `secret_file` (rejected: two credentials for one port); derive secret from cluster UUID (rejected: UUID is public identity).

## R3. Explicit bootstrap vs join in config

- **Decision**: Exactly one of `cluster { bootstrap; }` or `cluster { join; }` is required (startup error if both, neither, or bootstrap with a foreign seed). Seeds: `cluster { seeds { name N; address A; port P; } }` (rename of `004` `peers` for first-join discovery; `peers` remains accepted as an alias). Empty or self-only seeds + `bootstrap` → create UUID + secret. `join` without a reachable seed and **without** persisted membership → refuse `ready` (FR-008). Persisted membership ignores seeds as a restart gate (FR-016).
- **Rationale**: Spec FR-005/FR-008/FR-016. `004` `peers` was discovery; membership view replaces it after admit.
- **Alternatives considered**: Implicit bootstrap if seeds empty (rejected: two isolated processes would silently fork); always require seeds (rejected: last-node restart).

## R4. Identity directory

- **Decision**: `{data_dir}/identity/node.json` = `{ node_id: UUID, node_name: string }` created on first start (id random UUID, name from `node { name; }`). `{data_dir}/identity/cluster.json` = `{ cluster_uuid, cluster_name, secret_epochs: [{epoch, secret_hash or wrapped secret}] }` created at bootstrap or after first successful admit/token. Restart loads these files; missing cluster file on a `join` node means first join.
- **Rationale**: Node identity independent of hostname (FR-003). Cluster UUID never regenerated (`006` cluster store also holds it; local file is the restart cache).
- **Alternatives considered**: Identity only in Raft (rejected: cannot join or restart last node without a local copy); hostname as id (rejected: spec).

## R5. Pending join is not a member

- **Decision**: After secret + identity + unique name + full ladder are accepted, append `PendingJoin` to the cluster log. Pending rows are **not** in `members`. Admin `Admit { node_id }` appends `AdmitMember` (moves pending → member). A valid unused token appends `AdmitMember` in one step (no pending row, or pending is created and immediately admitted atomically). Pending MUST NOT vote, MUST NOT be a replica target, MUST NOT reach `ready` as a member. Process local state stays `starting` (`001`) until admitted (or the operator stops it).
- **Rationale**: Clarify 2026-09-18 Q5; FR-006/007/013. `006` `MemberRecord.status = joining` is **removed**; pending is a separate table so membership view stays “members only”.
- **Alternatives considered**: Pending as member with `joining` (rejected: would be a replica target/voter risk); token-only (rejected: clarify option A).

## R6. One-time join tokens

- **Decision**: `CLUSTER_ADMIN` (first binary: admin bearer) mints `JoinToken { id, node_id?, node_name, expires_at, used }`. Default TTL **12 hours** (range 1–72 h). Bound to name and optional id; single use; consumed on successful `AdmitMember`. Used or expired → refuse + audit (`014`). Token skips a second admit.
- **Rationale**: Spec “hours-scale TTL”. 12 h is enough for a rolling provision without living forever.
- **Alternatives considered**: No TTL (rejected: stolen token); bind only to name (weaker; still allowed if id omitted at mint, then bound at first use).

## R7. Ladder at join, addresses later

- **Decision**: Join payload includes every key on the cluster topology ladder (`004`). Omit or hierarchy-integrity failure → `JoinRefused { code: ladder }` and **no** pending row. After membership, listen-address/hostname changes are `MemberUpdate` (identity unchanged). Relabel of ladder keys is `004` live label change, not a re-join.
- **Rationale**: FR-006; constitution VII. Address change edge in spec.
- **Alternatives considered**: Fill missing ladder keys with defaults (rejected: spec refuse); require re-join on IP change (rejected: identity must not change).

## R8. Secret rotation overlap

- **Decision**: Operator two-phase: `secret-rotate begin` publishes epoch N+1; internodes accept **both** N and N+1; `secret-rotate complete` drops N. Optional `max_overlap` default **24 h** — if complete is not called, N dies automatically. Verify is constant-time over the currently accepted epoch list.
- **Rationale**: Spec requires an overlap window; an operator-driven complete avoids surprising cutover; a max overlap bounds forgotten rotations.
- **Alternatives considered**: Timer-only (no explicit complete; worse operator control); single-epoch cutover (rejected: rolling restart of members would race).

## R9. Operator drain vs stop

- **Decision**: Admin `Drain` sets member `status=draining` and local `NodeState::draining` but **does not exit**. Undrain sets `ready` and re-opens tenant accept. `001` `stop` / SIGTERM still drain-then-exit. Rolling restart uses stop. Live decommission requires operator drain (process up) so `004` can copy replicas off that node. A stop during decommission: remaining copy uses other replicas or the data-loss accept (spec edge).
- **Rationale**: Clarify Q3 option A; FR-010/011. `001` FR-006 remains the stop path.
- **Alternatives considered**: Drain always exits (rejected: RF=1 copy-off); new process state name `operator_draining` (unnecessary; reuse `draining` plus `exit_after_drain: bool`).

## R10. Decommission and retired identities

- **Decision**: `Decommission { node_id, accept_data_loss: bool }` (CLUSTER_ADMIN): require draining if the process is up; call `004` rebalance until constraints satisfied or `accept_data_loss` for unreplicated leftovers; then `RemoveMember` + `RetireIdentity { node_id }`. Retired ids are kept in cluster state. First join presenting a retired id → `RetiredIdentity`. Node **name** is freed (unique among current members only).
- **Rationale**: FR-011/018; clarify Q4 option A.
- **Alternatives considered**: Reuse id as ordinary join (rejected: looks like replace); never reuse names (rejected: operator “delete and add”).

## R11. Replace eligibility and incarnation

- **Decision**: `Replace { dead_id, new_process identity=dead_id }` allowed iff `012` failure detector currently marks that member **unavailable** (heartbeat timeout elapsed) **and** the caller is CLUSTER_ADMIN. No extra ping. Heartbeating `ready`/`draining` → `LiveReplace`. On commit: `incarnation += 1` on that `MemberRecord`; internodes `FenceIncarnation { node_id, min_incarnation }`. Old process with stale incarnation is closed on `internode`/`replication` and must not vote. Placements keep naming `dead_id`.
- **Rationale**: Clarify Q2 option B; FR-012. Incarnation is the fence so a partitioned “unavailable” node cannot return as a second live copy.
- **Alternatives considered**: Operator mark-dead bit (rejected: extra liveness check); auto-replace without admin (rejected: still need a new process and authorization).

## R12. First-binary voter set on join/leave

- **Decision**: Follow `006` R4: bootstrap voter `{A}`; second member learner; third member expands to `{A,B,C}`; further members learners. **Decommission of a learner**: remove from members/learners only. **Decommission of a voter** in first binary (3→2 members): committed voter set becomes **one remaining voter**; the other remaining member is a **learner** (odd size 1). Next admit of a third member expands to 3 voters again. **Replace** of a voter keeps the voter slot (same id, new incarnation). Slice 7 still owns explicit voter migrate/grow/shrink APIs.
- **Rationale**: Even voter sets are illegal in `006`; SC-005 still decommissions a three-node cluster.
- **Alternatives considered**: Refuse voter decommission until slice 7 (fails SC-005); 3→2 both vote (even; rejected).

## R13. Restart without seeds

- **Decision**: If `cluster.json` + membership snapshot (applied cluster log / identity dir) show this `node_id` is a **member**, start as that member: open Raft as voter or learner per store, become `ready` even if every seed is down. Do not require admit/token. If not a member and not bootstrap and no seed → not `ready` (FR-008).
- **Rationale**: Clarify Q1 option A; SC-007; `006` restore step 6.
- **Alternatives considered**: Always contact a seed (bricks last-node and simultaneous restart).

## R14. Internode handshake messages

- **Decision**: Additive internodes types (unknown types ignored per `004`): `JoinRequest`, `JoinAck`, `PendingAnnounce`, `FenceIncarnation`, `SecretRotate`. Membership **view** after admit is the cluster log / Raft apply, not a second gossip document. JoinRequest is allowed from a non-member that presents a currently valid secret; other cluster RPCs from non-members are `NotMember`.
- **Rationale**: FR-007/009; `012` secret-before-protocol.
- **Alternatives considered**: Piggyback join on `CatalogDelta` (rejected: non-member must not apply catalog); dedicated `join` handler/port (rejected: Principle V extra port).

## R15. Audit and CLUSTER_ADMIN

- **Decision**: Admit, token mint/use/refuse, decommission, replace, secret-rotate emit audit records with the `014` shape (principal id, action, target, HLC). Until `014` ships, write the same records to an in-memory ring + tracing (`membership.audit`) so SC-009/token-refuse tests exist. Authorization: admin bearer (`001`) is CLUSTER_ADMIN in the first binary.
- **Rationale**: Spec audit-logged tokens; `014` owns durable audit.
- **Alternatives considered**: Block until `014` (rejected: first-binary join).

## R16. Metrics names

- **Decision**: This crate increments (exposition in `08`): `spacestorage_membership_members`, `spacestorage_membership_pending`, `spacestorage_membership_join_total{result}`, `spacestorage_membership_replace_total{result}`, `spacestorage_membership_decommission_total{result}`, `spacestorage_membership_secret_epoch`. Labels `node`, `cluster_uuid`. Do not invent `08` replacements.
- **Rationale**: Principle X; `08` catalog stays source of names.
- **Alternatives considered**: No metrics until `08` (rejected: conformance SC counters in `004`/`006` already exist per crate).
