# Implementation Plan: Authentication, Authorization, Encryption in Transit, Keys, and Audit

**Branch**: `014-authz-keys` | **Date**: 2026-09-19 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/014-authz-keys/spec.md` (Clarifications, Sessions 2026-09-15, 2026-09-16, 2026-09-18, 2026-09-19 — TLS not globally mandatory; master-key file wraps namespace KEKs; encryption opt-in; renameable logins; unbound `CLUSTER_ADMIN` refused on Redis/S3/WebDAV/ES; bootstrap creates first admin; first-binary cut; `CLUSTER_ADMIN` implies all verbs; live sessions re-check on later requests; SCRAM default 16384; FR-017 implicit tenant grant)

## Summary

Own the **security mechanisms** that `007` left as names and opaque blobs: a cluster-wide **principal store** (immutable id, unique renameable login, SCRAM-SHA-256 verifier), a closed **permission vocabulary** with `CLUSTER_ADMIN` implying every other verb, **envelope keys** (cluster master-key file wrapping per-namespace KEKs in the cluster log; data keys for containers they host), and a durable **audit log** of privileged actions. Protocol handlers keep native exchanges (`002`); they resolve credentials through this crate's `Authenticator`, replacing the interim `auth.users_file`. `crates/crypto` `KeyAuthority` stops being a keyring file and becomes this envelope. Transport `tls` / `plaintext;` stays `001`; this feature adds optional mTLS mapping and keeps the no-silent-fallback rule.

**First binary** (`016` slices 1–5): local principal store; bootstrap `CLUSTER_ADMIN`; SCRAM + Redis AUTH; one-namespace binding; built-in `admin`=`{CLUSTER_ADMIN}` and `replication`=`{REPLICATE}` only (not editable); implicit data-plane grant for bound tenant principals; every entrypoint already `tls` or `plaintext;` (`001`); master-key file wrapping KEKs; audit of join / key bind-rotate / failed auth / login rename / namespace rename, readable by `CLUSTER_ADMIN`. **Slice 7** (with `007` `tenancy-quotas`): custom roles replace the implicit tenant grant, remaining verbs as grants, tenant `AUDIT_READ`, `NAMESPACE_ADMIN` key bind and login rename. **`010`**: rewrite existing data under a new data key. LDAP/SSO and external KMS later.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`, `subtle`, `zeroize`, `rustls`/`tokio-rustls`). SCRAM: `pgwire` `server-api-scram` already in `002` plus `hmac`/`sha2`/`pbkdf2` (pure Rust) for Redis AUTH mapping to the same verifier. Envelope: existing `aes-gcm`, `chacha20poly1305`, `hkdf` in `003` `crypto`. No OpenSSL, no KMS SDK, no LDAP.

**Storage**: Principal, KEK (wrapped), audit, and session-token hash rows are **cluster-log** bodies via `controlplane` (`{data_dir}/raft/cluster/`), same store as `007` roles/bindings. Master key is a **0600 file** referenced by path (`keys { master_key_file P; }`), never inlined, backupable by copying the file. Data-key wrapped blobs live with the container definition (namespace Raft, `003`) plus a `KeyRef` id. Unwrapped data keys are **process memory only** (`Zeroizing`), cached for hosted containers; `CLUSTER_ADMIN` MAY unwrap for restore/admin outside that hosted-only cache. Not tenant WAL.

**Testing**: `cargo test`. Unit: SCRAM verify, implication, generation bump, unbound Redis refuse, rewrap, audit fields. `crates/conformance`: bootstrap admin (SC-008); PG+Redis bind (SC-001); password change later-request refuse (SC-009); login rename (SC-007); join/key-rotate/failed-auth audit (SC-006); omit transport / TLS refuse plaintext (SC-003, already `001`); encrypted snapshot unreadable without master (SC-004); master rotate 0 rewrites (SC-005); custom-role create refused on first-binary profile (SC-002). Slice 7 feature `authz-custom`. Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node tests: in-process, loopback, same harness as `011`/`016`.

**Project Type**: Cargo workspace extension — **one new library crate** `crates/authz` (`spacestorage-authz`). Envelope `KeyAuthority` implementation lands in existing `crates/crypto` (no new crypto crate). No new binary. Replaces `002` users-file authenticator and `003` interim `keyring_file`. Additive admin/CLI/config. `release-profile`: first binary compiles principal store + envelope + audit append; `authz-custom` is slice 7.

**Performance Goals**: SCRAM verify < 5 ms p95 (PBKDF2 iterations documented; normative default **16384**, tests MAY use **4096**, production MAY raise up to 65536). Permission check after session bind < 5 µs p99 (bitmask). Later-request re-check is a generation compare + verb bit, not a new SCRAM. Master rewrap of tens of namespace KEKs < 1 s. Leaderless KV p95 unchanged vs `004` when the principal is already authenticated.

**Constraints**: Hostile tenants; operator runs all nodes (crash-stop). Unbound `CLUSTER_ADMIN` refused on Redis/S3/WebDAV/ES. Join secret MUST NOT authenticate as admin. Empty principal store MUST NOT leave admin open. Built-in roles immutable. `CLUSTER_ADMIN` implies all verbs. Later requests re-check; no forced disconnect. Key material never in logs/describe. First binary MUST NOT require custom roles, tenant `AUDIT_READ`, KMS, or data-key rewrite. UIs (`09`) get no extra verbs.

**Scale/Scope**: Hundreds of principals; tens to hundreds of namespaces (one KEK each); a handful of data keys per encrypted container (current + retained). Audit is an append-only cluster log (retain by Raft snapshot + optional `08` export). Roughly: principal/SCRAM/session (~3 k), permission vocabulary (~1 k), envelope KeyAuthority (~2.5 k), audit (~1.5 k), admin/config (~1.5 k), conformance (~3 k); ≈ 12–16 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | `authz` + envelope in `crypto`; rustls; RustCrypto AEAD; no OpenSSL/KMS/`*-sys` | PASS |
| II | Fully Asynchronous Tokio Runtime | Auth and unwrap are `async`; PBKDF2/SCRAM on `spawn_blocking` when iterations are high; file reads via `tokio::fs` | PASS |
| III | Single-Process Multithreaded Monolith | Library in `spacestoraged`; no auth sidecar, no HSM process | PASS |
| IV | Type-Driven Multiparadigm | No new datatype. Encryption algorithms stay `003` inventory | PASS |
| V | Protocol Compatibility on Distinct Ports | Native SCRAM/AUTH/SASL stay on existing handlers; no SpaceStorage-native client protocol | PASS |
| VI | Every Node Is a Request Coordinator | Any member verifies principals from cluster-applied state; no mandatory auth proxy | PASS |
| VII | Label-Based Planetary Placement | Unchanged. Data keys follow hosted containers (`004` pin) | PASS |
| VIII | Cassandra-Style Quorum | Unchanged. Authz is metadata (cluster Raft), not write-ack arithmetic | PASS |
| IX | Multi-Tenant Namespaces | One-namespace binding for non-admin; `CLUSTER_ADMIN` selects ns on native-select protocols only | PASS |
| X | Observability as a Product Surface | Increments reserved `spacestorage_auth_*` / audit counters; `08` names not renamed. Audit **content** is this crate; `08` optionally exports | PASS |
| XI | Documented, Expandable Configuration | `bootstrap` admin login/password file; `keys.master_key_file`; starters | PASS |
| XII | Raft Controller Elections and Local Restore | Principal/KEK/audit in cluster log; restore = replay. Data path still leaderless | PASS |
| XIII | Security Defaults for Data and Roles | **This feature is the principle.** Envelope per container; roles in cluster storage; vocabulary owned here, names stored in `007` | PASS |
| Arch. Contracts | Five-level stack | Authz is not a type. Handlers still `002` → exec. `KeyAuthority` stays the crypto seam | PASS |
| Observability Contract | No `08` series renamed | Additive auth/audit/key figures only | PASS |

**Gate result (pre-research)**: PASS. Sequenced deviations (replace `001` admin token and `002`/`003` interim files; audit SoT in cluster log not `08`; mTLS optional) are in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/014-authz-keys/
├── plan.md
├── research.md                     # Phase 0: R1–R17
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── principals.md               # store, login rename, password, disable
│   ├── permissions.md              # vocabulary, implication, builtins
│   ├── authenticator.md            # replaces 002 users_file; protocol mapping
│   ├── sessions.md                 # generation re-check; bearer
│   ├── tls.md                      # consume 001 tls/plaintext; optional mTLS
│   ├── keys.md                     # envelope KeyAuthority; rotate; restore
│   ├── audit.md                    # cluster-log SoT; events; AUDIT_READ
│   ├── config-directives.md
│   ├── admin-cli.md
│   ├── metrics.md
│   ├── protocol-mapping.md         # PG/Redis/admin; unbound refuse
│   └── fixtures/
│       ├── README.md
│       ├── bootstrap-admin.conf
│       ├── master-key.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── authz/                                   # spacestorage-authz — NEW
│   └── src/
│       ├── lib.rs                           # Authenticator, Authorizer, AuditLog
│       ├── principal.rs                     # PrincipalRecord, login index, generation
│       ├── scram.rs                         # SCRAM-SHA-256 verifier; Redis AUTH map
│       ├── permission.rs                    # Verb bitmask; CLUSTER_ADMIN implies all
│       ├── session.rs                       # cred_generation; later-request check
│       ├── bootstrap.rs                     # first CLUSTER_ADMIN from config
│       └── audit.rs                         # AuditEntry; cluster-log append + read
│
├── crypto/                                  # OWNED jointly with 003
│   └── src/
│       ├── key_authority.rs                 # EnvelopeAuthority implements KeyAuthority
│       ├── envelope.rs                      # master wrap/unwrap KEK; KEK wrap data key
│       └── master_file.rs                   # 0600 master_key_file; backup copy
│
├── tenancy/                                 # Role / RoleBinding rows; call authz verbs
├── controlplane/                            # cluster-log bodies: Principal*, Kek*, Audit*
├── handler-postgresql/ | handler-redis/     # Authenticator (drop users_file)
├── internodes/ | membership/                # REPLICATE + join secret; audit admit
├── config/                                  # cluster.admin_login; keys.master_key_file
├── node/                                    # refuse start if no bootstrap admin / no master when encrypting
├── admin-proto/ | spacestorage/             # principals, keys, audit CLI
├── release-profile/                         # first binary: authz; authz-custom slice 7
└── conformance/                             # SC-001–SC-009
```

**Structure Decision**: New `authz` crate so `tenancy` stays registry/quotas/bindings and `controlplane` stays Raft. Envelope lives in `crypto` because `003`/`013` already call `KeyAuthority` on the write path; swapping the provider must not move AEAD. Audit is a module in `authz` (one privileged-action vocabulary) with cluster-log durability; `08` only optionally ships those entries.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Replace `001` `admin { token_file }` with bootstrap principal | Spec FR-015: first `CLUSTER_ADMIN` from bootstrap login+password; empty store must not leave admin open | Keeping the static token would leave join-secret-adjacent "anyone with the file is admin" and contradict bootstrap-admin |
| Replace `002` `auth.users_file` and `003` `keyring_file` | Spec requires cluster principal store and envelope KEKs | Leaving interim files would ship two credential/key systems in the first binary |
| Audit source of truth in cluster log, not `08` | Clarify deferred; join/key-rotate must survive node death in the first binary before slice 9 sinks | `08`-only audit is off by default and not in the first binary |
| Every node with the master file can unwrap any KEK (policy limits to hosted containers) | Operator runs all nodes; first binary has no per-node wrapping keys | Per-node wrapping would need a KMS or threshold keys (explicitly later) |
| Implicit first-binary `{READ,WRITE,CREATE,DROP,CONFIGURE}` for bound tenants (**FR-017**) | Redis AUTH smoke cannot use unbound `admin` (clarify 2026-09-18 Q1 / 2026-09-19); custom roles wait for slice 7 | Shipping custom RolePut in slices 1–5 would pull `007` quotas/custom into the first binary |
| Data-key rewrite via `010` | Spec FR-010 | Re-encrypting tables in slice 2 would pull the transform engine into the first binary |

## Constitution Check (post-design)

Re-evaluated after Phase 1: principals/KEKs/audit on the cluster log; SCRAM/Redis AUTH map to one store; unbound admin refused on credential-bound protocols; bound tenants get an implicit data-plane grant until slice 7; `CLUSTER_ADMIN` implies all verbs; built-ins immutable; envelope replaces keyring; later requests re-check generation; `08` does not own audit content; handlers stay native-auth adapters. **Gate result: PASS.**
