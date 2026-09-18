# Quickstart: Cluster identity, join, drain, replace

**Feature**: `011-identity-membership` | **Gates**: SC-001–SC-009 | First binary slices 1–5 (`016`)

Prerequisites: loopback internodes from `004`/`012`; admin token as `001`; topology ladder default `[az]` (`004`/`016`). Commands: [admin-cli.md](contracts/admin-cli.md). Model: [data-model.md](../data-model.md).

Configs: [bootstrap.conf](contracts/fixtures/bootstrap.conf), [join-pending.conf](contracts/fixtures/join-pending.conf), [join-token.conf](contracts/fixtures/join-token.conf).

## 1. Bootstrap the first node (SC-001, SC-002)

Start `db-1` with `cluster { bootstrap; }` and empty seeds. Expect: `ready`; `spacestorage membership` shows one member; `join.secret` exists; cluster UUID printed once.

Start a **second isolated** process with the same cluster **name** and `bootstrap`. Expect: a **different** UUID; they MUST NOT merge if later networked.

## 2. Pending join then admit (SC-003, SC-004, SC-009)

Start `db-2` with [join-pending.conf](contracts/fixtures/join-pending.conf) (secret, no token). Expect: process stays `starting`; `membership` lists `db-2` under **pending**, not members; it MUST NOT take replica placements or Raft votes.

```text
spacestorage admit <db-2-id>
spacestorage membership
```

Expect: `db-2` is a member, `ready`, may receive client requests. Existing container replica maps unchanged.

Omit `az` ([join-missing-ladder.conf](contracts/fixtures/invalid/join-missing-ladder.conf)) → join refused, no pending row.

Wrong secret → not a member, 0 replica targets.

## 3. Token join (SC-009)

On `db-1`:

```text
spacestorage join-token mint --name db-3 --ttl 12h
```

Start `db-3` with [join-token.conf](contracts/fixtures/join-token.conf). Expect: member without a second admit. Reuse the same token → refuse, audit line.

## 4. Operator drain, undrain, live decommission (SC-005)

Three-node RF=2. On `db-3`:

```text
spacestorage drain db-3
```

Expect: process still running; no new tenant connections; no new replica placements; internodes still up.

```text
spacestorage undrain db-3
```

Expect: `ready` again.

```text
spacestorage drain db-3
spacestorage decommission db-3
```

Expect: replicas copied off `db-3` onto remaining members (or named blockers); `db-3` gone from members; name `db-3` reusable; identity retired. Voter set becomes 1 voter + 1 learner ([research R12](../research.md)).

`001` stop on a member still exits the process (rolling-restart path).

## 5. Replace after failure-detector timeout (SC-006)

Kill `db-2` without decommission. Wait until internodes failure timeout (`004`/`012` `failure_timeout`). Then start a new process with **the same node id** and `JoinRequest.replace_of`. Expect: replace succeeds; placements still name that id; the killed process, if restarted with stale incarnation, is fenced.

Replace while `db-2` is still heartbeating → `live_replace`.

## 6. Restart without seeds (SC-007)

Stop all three; start them with **empty seeds** and the same `data_dir`. Expect: each becomes `ready` as the same member; no new admit/token. Decommission one that will not return; join a fourth with a **new** id (name may reuse). Survivors are not re-admitted.

## 7. Name reuse vs retired identity (SC-008)

After decommission of `db-3`, join a new node named `db-3` with a new UUID → accepted. Join presenting the retired UUID → `retired_identity`.
