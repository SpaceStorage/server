# Implementation Plan: SpaceStorage Runtime, CLI, and Node Interfaces

**Branch**: `001-runtime-cli-api` | **Date**: 2026-09-13 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/001-runtime-cli-api/spec.md`

## Summary

Build the SpaceStorage node process skeleton: one Tokio multi-thread process sized to the machine's available cores, an nginx-style block configuration with an `entrypoint { port; handler; }` model where every listening port speaks exactly one handler, two built-in admin handlers (`admin` over a length-prefixed JSON TCP protocol, `admin-http` over HTTP/JSON) that must be explicitly enabled or disabled per node, optional per-entrypoint TLS via rustls with file-referenced certificates, a named-buffer registry with live-reloadable capacities and reject/wait overflow policies, graceful drain/stop, and a bundled `spacestorage` CLI (`clap`) that validates configs offline and talks to a node over either admin handler. All crates are Rust; no C/OpenSSL dependencies. Metric exposition and role-based auth are owned by later features (`08`, `07`); this feature tracks the figures and ships a file-referenced static admin token as the interim authentication so FR-026 holds from day one.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85). Locally installed toolchain is 1.87.0.

**Primary Dependencies**: `tokio` (multi-thread runtime, `net`, `signal`, `sync`, `time`, `fs`), `axum` + `hyper` 1.x (admin-http), `rustls` + `tokio-rustls` + `rustls-pemfile` (TLS, pure Rust), `serde` + `serde_json` (admin protocol, effective config, CLI JSON output), `clap` (CLI, derive), `tracing` + `tracing-subscriber` (logging), `bytes`. Config grammar parser is hand-written (recursive descent) inside the `config` crate — no external grammar crate. Client TLS in the CLI uses `rustls` with `webpki-roots` plus an optional operator CA file.

**Storage**: N/A. This feature persists nothing; configuration is read from a file. Buffers are in-memory only.

**Testing**: `cargo test` — unit tests per crate; integration tests in `crates/node/tests/` that boot a node in-process on ephemeral ports and drive it over `admin` and `admin-http`; contract tests that parse every fixture under `specs/001-runtime-cli-api/contracts/fixtures/` and compare against expected validation output; CLI tests via `assert_cmd`-style process spawning.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. `available_parallelism()` respects cgroup CPU quotas and affinity masks on Linux.

**Project Type**: Single Cargo workspace producing two binaries (`spacestoraged` server, `spacestorage` CLI) and library crates.

**Performance Goals**: SC-001 `ready` in < 10 s (target < 1 s); SC-003 ≥ 99% of short admin requests complete within 2× unloaded latency while one long request is held; SC-005 all entrypoints accept within 1 s of `ready`; SC-009 zero dropped connections on reload; SC-010 drain completes within timeout + 1 s.

**Constraints**: No blocking calls on Tokio worker threads (file reads via `tokio::fs`/`spawn_blocking`, TLS handshakes async); every listener is declared; certificate material referenced by path, never inlined; no plaintext fallback for TLS entrypoints; reload is atomic (validate whole config, then swap `Arc<EffectiveConfig>` and adjust buffer capacities); default listen address `127.0.0.1`; default drain timeout 30 s.

**Scale/Scope**: One node process; tens of entrypoints; 3 built-in buffers in this feature (`net.recv`, `net.send`, `request.queue`) with a registry designed for hundreds; admin API is low-QPS (operators and scrapers), but the runtime must not degrade under thousands of concurrent protocol connections later. Roughly 5 crates, ~6–8k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | All workspace crates and transitive deps are Rust; TLS via `rustls` (no OpenSSL/`native-tls`); parser hand-written | PASS |
| II | Fully Asynchronous Tokio Runtime | One `tokio` multi-thread runtime; config/cert file I/O via `tokio::fs`; no `std::net` blocking accept; `spawn_blocking` only for `available_parallelism`-class syscalls at startup | PASS |
| III | Single-Process Multithreaded Monolith | One process (`spacestoraged`); `worker_threads` = configured or `available_parallelism()`; CLI is a separate binary but shipped in the same distribution and never required for the node to run | PASS |
| IV | Type-Driven Multiparadigm | Feature introduces no datatypes; the `Handler` trait is a protocol-adapter slot (wire → future abstract type interface), not a type hierarchy | PASS (N/A) |
| V | Protocol Compatibility on Distinct Ports | Entrypoint model enforces exactly one handler per address:port; duplicate address:port is a startup error | PASS |
| VI | Every Node Is a Request Coordinator | Not constrained by this feature; admin handlers run on every node | PASS (N/A) |
| VII | Label-Based Planetary Placement | `node { name; }` present; labels reserved for `04`/`06`; config grammar accepts unknown top-level blocks only via explicit feature registration, so labels can be added without grammar changes | PASS (N/A) |
| VIII | Cassandra-Style Quorum | Not applicable to admin handlers | PASS (N/A) |
| IX | Multi-Tenant Namespaces | Admin API is node-scoped and namespace-agnostic; buffer stats carry `node` label only, `namespace` labels added by `08` | PASS (N/A) |
| X | Observability as a Product Surface | Buffer usage (bytes, percent, limit hits), worker thread count/busy, uptime, node_state tracked in memory via a `stats` module with Prometheus-conformant names reserved (`spacestorage_buffer_usage_bytes`, …); exposition endpoint owned by `08` | PASS |
| XI | Documented, Expandable Configuration | `quickstart.md` + `contracts/config-grammar.md` + `contracts/fixtures/node.conf` starter example; handler and buffer inventories are registries | PASS |
| XII | Raft Controller Elections and Local Restore | Not applicable; `ready` is local-only in this feature and later features add preconditions | PASS (N/A) |
| XIII | Security Defaults for Data and Roles | Role system does not exist yet (`07`). Interim: admin handlers require a bearer token read from a file referenced in config (`admin { token_file …; }`); rejected without it. Loopback default bind and per-entrypoint TLS. Tracked in Complexity Tracking. | PASS with justified deviation |
| Arch. Contracts | Four-level stack respected; protocol adapter ≠ type system | Handler trait only translates wire to a future `Node` service interface; no data types defined | PASS |
| Observability Contract | Labels/series from `08` are not renamed | This feature only reserves names and defers exposition | PASS |

**Gate result (pre-research)**: PASS. One justified deviation (XIII interim auth) recorded below.

## Project Structure

### Documentation (this feature)

```text
specs/001-runtime-cli-api/
├── plan.md                         # This file
├── research.md                     # Phase 0 output
├── data-model.md                   # Phase 1 output
├── quickstart.md                   # Phase 1 output
├── contracts/                      # Phase 1 output
│   ├── config-grammar.md           # nginx-style block grammar + directive reference + validation errors
│   ├── admin-api.md                # Shared admin operations, JSON schemas, error codes
│   ├── admin-tcp-protocol.md       # Framing for the `admin` handler
│   ├── admin-http.md               # Routes for the `admin-http` handler
│   ├── cli.md                      # `spacestorage` command tree, flags, exit codes
│   └── fixtures/                   # Config fixtures used by contract tests
│       ├── node.conf               # Starter example (SC-011)
│       ├── minimal.conf
│       └── invalid/*.conf          # One file per SC-004 rejection case
├── checklists/requirements.md
└── tasks.md                        # Phase 2 output (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
Cargo.toml                      # workspace
rust-toolchain.toml             # 1.87
crates/
├── config/                     # spacestorage-config
│   └── src/
│       ├── lib.rs
│       ├── lexer.rs            # tokens: ident, number, string, `{` `}` `;`
│       ├── parser.rs           # recursive descent → AST (Directive | Block)
│       ├── ast.rs
│       ├── model.rs            # NodeConfig, EntrypointDecl, TlsDecl, BufferDecl, AdminDecl
│       ├── resolve.rs          # AST → NodeConfig with defaults, overrides; collects ALL errors
│       ├── validate.rs         # cross-field rules: admin declarations, dup addr:port, ranges
│       ├── reload_class.rs     # Live | RestartRequired per setting; diff of two configs
│       └── error.rs            # ConfigError { path, line, col, setting, message }
├── admin-proto/                # spacestorage-admin-proto (shared by node + CLI)
│   └── src/
│       ├── lib.rs
│       ├── types.rs            # Status, EffectiveConfig, BufferReport, ReloadReport, ErrorBody
│       ├── ops.rs              # enum AdminOp { Status, Config, Threads, Buffers, Reload, Stop }
│       └── frame.rs            # length-prefixed JSON codec for the `admin` handler
├── node/                       # spacestorage-node (library: the runtime)
│   ├── src/
│   │   ├── lib.rs
│   │   ├── runtime.rs          # build tokio runtime from NodeConfig (worker_threads)
│   │   ├── lifecycle.rs        # NodeState machine, ready/drain/stop, signals
│   │   ├── entrypoint/
│   │   │   ├── mod.rs          # bind all listeners before ready; unbind on abort
│   │   │   ├── listener.rs     # accept loop → handler.serve(conn)
│   │   │   └── tls.rs          # rustls acceptor from file refs; validity check
│   │   ├── handler/
│   │   │   ├── mod.rs          # Handler trait + HandlerRegistry (inventory)
│   │   │   ├── admin_tcp.rs    # `admin` handler
│   │   │   └── admin_http.rs   # `admin-http` handler (axum router)
│   │   ├── admin/
│   │   │   ├── mod.rs          # AdminService: executes AdminOp against the Node
│   │   │   └── auth.rs         # token_file bearer check (interim until 07)
│   │   ├── buffer/
│   │   │   ├── mod.rs          # BufferRegistry, Buffer, OverflowPolicy
│   │   │   └── builtin.rs      # net.recv, net.send, request.queue
│   │   ├── reload.rs           # reload orchestration: parse → validate → diff → apply live
│   │   ├── stats.rs            # in-memory counters/gauges; names reserved for 08
│   │   └── effective.rs        # EffectiveConfig snapshot (Arc swap), pending-restart markers
│   └── tests/
│       ├── startup.rs          # SC-001, SC-002, SC-004, SC-005
│       ├── admin_parity.rs     # same answers over admin and admin-http
│       ├── tls.rs              # SC-006
│       ├── reload.rs           # SC-009, FR-040
│       ├── drain.rs            # SC-010, FR-006/007
│       └── buffers.rs          # SC-008, FR-042
├── spacestoraged/              # binary: the server
│   └── src/main.rs             # parse args (--config, --set key=value), build runtime, run node
└── spacestorage/               # binary: the CLI
    ├── src/
    │   ├── main.rs
    │   ├── args.rs             # clap command tree (see contracts/cli.md)
    │   ├── client/
    │   │   ├── mod.rs          # AdminClient trait
    │   │   ├── tcp.rs          # admin handler client (frame codec)
    │   │   └── http.rs         # admin-http client
    │   ├── output.rs           # human table vs --output json
    │   └── exit.rs             # exit codes
    └── tests/cli.rs

docs/
├── configuration.md            # generated from contracts/config-grammar.md at release
└── examples/node.conf          # copy of contracts/fixtures/node.conf
```

**Structure Decision**: Cargo workspace with three library crates (`config`, `admin-proto`, `node`) and two binaries (`spacestoraged`, `spacestorage`). `config` has no Tokio dependency so the CLI can validate offline with the exact same code the node uses (FR-032). `admin-proto` is shared so the CLI and both admin handlers serialize the same types (FR-020 parity). Later features add handler crates (`crates/handler-postgresql`, …) that register into `node::handler::HandlerRegistry` and buffers into `node::buffer::BufferRegistry` (FR-017, FR-043).

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| XIII: interim static admin token instead of cluster role system | FR-026 requires unauthenticated callers be rejected now; the role system arrives in feature `07` (control-plane storage of roles) | "No auth, bind to loopback" leaves TLS-exposed admin entrypoints open on management networks and violates FR-026; implementing roles here would duplicate `07` and require controller storage that does not exist yet. The token is read from a referenced file (never inlined), checked in constant time, and the `admin::auth` module is the single seam `07` replaces. |
| Two binaries in one workspace | Spec requires a CLI shipped with the server (FR-031) that works without a running node (FR-032) | A single binary with `server`/`ctl` subcommands would pull the full runtime into every CLI invocation and blur the "node is one process" story; two binaries from one `cargo install`/package keep FR-031 and III both true. |

## Phase 0 — Research

See [research.md](research.md). All Technical Context items were resolvable from the spec, constitution, prior experiments in `../test/` (Tokio + rustls + prometheus listeners), and the local toolchain; no `NEEDS CLARIFICATION` remained after research.

## Phase 1 — Design

- [data-model.md](data-model.md): entities, fields, validation rules, state machines (node lifecycle, buffer admission, reload).
- [contracts/](contracts/): config grammar, admin API, TCP framing, HTTP routes, CLI command tree, fixtures.
- [quickstart.md](quickstart.md): validate → start → inspect → reload → stop walkthrough mapped to success criteria.

## Constitution Check (post-design)

Re-evaluated after Phase 1: no new violations. Design keeps a single runtime, all-Rust dependency set, one handler per port, file-referenced certificates and token, in-memory stats with `08`-compatible names, and documented starter configuration. The interim auth deviation stands as tracked above. **Gate result: PASS.**

## Next Step

Run `/speckit-tasks` to generate `tasks.md` from this plan and the Phase 1 artifacts.
