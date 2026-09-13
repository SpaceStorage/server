# Research: SpaceStorage Runtime, CLI, and Node Interfaces

**Feature**: `001-runtime-cli-api` | **Date**: 2026-09-13

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md), constitution, the earlier experiments in `../test/` (`server3` used Tokio + rustls + per-listener `handler`/`tls`/`buffer_size` YAML, `conf` used the `config` crate, `tracing` used `tracing-subscriber`), and the local toolchain (Rust 1.87).

## R1. Configuration file format

- **Decision**: nginx-style block grammar, hand-written recursive-descent parser in `crates/config`. Directives are `name arg…;`, blocks are `name arg… { … }`. Example the spec fixes: `entrypoint { port 9042; handler cassandra; }`.
- **Rationale**: The clarified spec uses this syntax literally (FR-016, SC-011). Repeated blocks (many `entrypoint {}`) are natural, comments are `#`, and errors can carry file:line:col plus the directive name, which FR-009 requires. A hand-written parser is ~300 lines, has no dependency, and gives full control over error recovery so *all* problems are reported in one pass.
- **Alternatives considered**: TOML/YAML via `serde` (rejected: contradicts the syntax fixed in the spec; YAML was used in `test/server3` but the user moved to block syntax); `pest`/`nom` grammar crates (rejected: pure Rust and acceptable, but add a dependency for a tiny grammar and make "collect all errors" harder than a hand-rolled parser).

## R2. Collecting all validation errors in one pass (FR-009, FR-032)

- **Decision**: Three phases, each accumulating into `Vec<ConfigError>`: lex/parse (syntax errors abort further parsing of that file but keep earlier errors), resolve (AST → typed model; unknown directive, wrong arity, bad literal each produce one error and the field is left at default so later checks continue), validate (cross-field: admin declarations present, duplicate `address:port`, handler known, buffer known and in range, thread count ≥ 1, TLS file references exist and are readable and not expired). Port-in-use and unowned-address are detected at bind time and reported through the same `ConfigError` type before `ready`.
- **Rationale**: SC-004 and Story 3 scenario 2 require three distinct problems to be listed in one run. Certificate existence/expiry checks require I/O; they run in validate for the CLI (blocking is fine there) and via `tokio::fs` in the node.
- **Alternatives considered**: Fail-fast on first error (rejected by spec); `serde`-based deserialization errors (rejected: stop at first error and lose position info).

## R3. Async runtime and thread sizing (FR-001–FR-004)

- **Decision**: `tokio::runtime::Builder::new_multi_thread().worker_threads(n).enable_all()`, where `n` = `runtime.threads` if configured else `std::thread::available_parallelism().map(NonZero::get).unwrap_or(1)`. Blocking pool (`max_blocking_threads`) left at Tokio default; used only for `spawn_blocking` around syscalls without async equivalents. Busy-thread count for FR-025 is derived from `tokio::runtime::RuntimeMetrics` (`num_workers`, per-worker `worker_busy_duration`/poll counts) sampled by `stats.rs`; when unstable metrics are unavailable on stable, fall back to a counter of in-flight admin/handler tasks.
- **Rationale**: `available_parallelism` honours cgroup quotas and affinity masks on Linux, matching the "cores available to the process" clarification. Runtime is built once in `spacestoraged::main` before the async world starts, so the thread count is a restart-required setting by construction (FR-012).
- **Alternatives considered**: `num_cpus` crate (rejected: redundant with std since 1.59); current-thread runtime with manual thread pool (rejected: contradicts Principle II/III).

## R4. Admin over TCP — framing for the `admin` handler (FR-020, FR-033)

- **Decision**: Length-prefixed JSON frames: `u32` big-endian payload length, then a UTF-8 JSON object. First frame from the client is a `hello` with `proto_version: 1` and the bearer token; the server answers `hello_ok` or `error` and closes on auth failure. Subsequent frames are `{ "id": n, "op": "status" | "config" | "threads" | "buffers" | "reload" | "stop", "args": {…} }` with responses `{ "id": n, "ok": true, "result": … }` or `{ "id": n, "ok": false, "error": { code, message } }`. Max frame 4 MiB. Codec lives in `admin-proto::frame` and is used by both the handler and the CLI.
- **Rationale**: Same JSON types as `admin-http` guarantee parity (FR-020) with one serialization path; length prefix makes future binary payloads and pipelining possible; trivially inspectable during debugging.
- **Alternatives considered**: newline-delimited JSON (rejected: fragile with embedded newlines and no size bound); `postcard`/`bincode` binary (rejected for v1: harder to debug, no parity benefit); reusing HTTP on both ports (rejected: the spec distinguishes `admin` TCP from `admin-http`).

## R5. Admin over HTTP (FR-020)

- **Decision**: `axum` 0.8 on `hyper` 1.x, JSON bodies, routes under `/v1/` (see `contracts/admin-http.md`): `GET /v1/status`, `GET /v1/config`, `GET /v1/threads`, `GET /v1/buffers`, `POST /v1/reload`, `POST /v1/stop`, `GET /v1/health/live`, `GET /v1/health/ready`. Bearer token in `Authorization` header (interim auth). `/metrics` is reserved for feature `08` and returns 404 in this feature.
- **Rationale**: axum is pure Rust, Tokio-native, and shares the `hyper` server that `08`'s `/metrics` and `02`'s Elasticsearch/S3/WebDAV HTTP handlers will reuse. Health endpoints map directly to `node_ready`/`node_state`.
- **Alternatives considered**: `warp` (used in `test/server3`; rejected: less active, filter-combinator style harder to extend); raw `hyper` service (rejected: more boilerplate, same dependency).

## R6. TLS (FR-027–FR-030)

- **Decision**: Server side `tokio-rustls` with `rustls` (ring or aws-lc-rs backend — choose `ring` for pure-Rust build simplicity), certificates and keys loaded with `rustls-pemfile` from paths given by `tls { certificate <path>; key <path>; }` inside an `entrypoint` block. At startup the node parses the leaf cert and fails if `not_after` < now or `not_before` > now (FR-028). Client side (CLI) `rustls` with `webpki-roots` plus `--ca <file>` to add an operator CA; no `--insecure` flag in v1 (FR-030). Expiry while running: a periodic (hourly) check flips an `expired` marker on the entrypoint report and logs; connections are left to fail at handshake.
- **Rationale**: Pure Rust (Principle I), async-native, the pattern already proven in `test/server3/src/listener/tcp/tls.rs`. Path references satisfy "referenced, never inlined"; a `vault:`-style reference form is reserved in the grammar for `07`.
- **Alternatives considered**: `native-tls`/OpenSSL (rejected: C dependency); mandatory TLS (rejected by clarification Q3); inline PEM in config (rejected by spec).

## R7. Default listen address and ports

- **Decision**: When an `entrypoint` omits `address`, use `127.0.0.1`. No default ports: `port` is mandatory on every entrypoint. Recommended (documented, not defaulted) ports: `admin` 7700, `admin-http` 7701.
- **Rationale**: Secure-by-default for admin surfaces; explicit `address 0.0.0.0;` (or a management IP) is a one-line, visible choice. Mandatory `port` keeps the declaration self-describing and avoids hidden collisions between handlers' conventional ports.
- **Alternatives considered**: `0.0.0.0` default (rejected: exposes admin by omission); per-handler default ports (rejected: hidden conflicts, e.g. two `admin` entrypoints).

## R8. Interim authentication for admin handlers (FR-026, Principle XIII)

- **Decision**: Top-level `admin { token_file <path>; }` directive is mandatory whenever at least one admin handler is enabled. The file is read at startup (and on reload), trimmed, and compared in constant time against the `Authorization: Bearer` header (`admin-http`) or the `hello.token` field (`admin`). Missing/unreadable file or empty token is a startup error. Health endpoints (`/v1/health/*`) are unauthenticated and return only `state`. The check is isolated in `node::admin::auth` so `07` can swap in the role system.
- **Rationale**: Meets FR-026 without inventing roles. Token lives outside the config file, satisfying "never inlined".
- **Alternatives considered**: No auth + loopback (rejected: violates FR-026); mTLS-only (rejected: forces TLS which Q3 made optional); implementing roles here (rejected: belongs to `07`).

## R9. Buffer registry and overflow semantics (FR-038–FR-044)

- **Decision**: `Buffer { name, capacity: AtomicU64, used: AtomicU64, limit_hits: AtomicU64, policy: Reject | Wait, range: RangeInclusive<u64>, default }`. `reserve(n) -> Result<Permit, Overflow>` under `Reject`; `reserve(n).await` under `Wait` using a `tokio::sync::Notify` woken on `release`. Lowering capacity below `used` is allowed: `reserve` fails/waits until `used + n <= capacity`; `used` may exceed `capacity`, so percent can exceed 100 (FR-040). Built-ins: `net.recv` (default 64 MiB, range 1 MiB–64 GiB, Wait), `net.send` (default 64 MiB, same range, Wait), `request.queue` (default 16 MiB, range 1 MiB–16 GiB, Reject). Registry is `HashMap<&'static str, Arc<Buffer>>` built at startup from built-ins plus features' `register()` calls; per-entrypoint attribution is tracked as `used_by_entrypoint` gauges in `stats.rs`. Sum-of-capacities vs available memory warning uses `/proc/meminfo` (`MemAvailable`) on Linux and `sysctl hw.memsize` on macOS via `spawn_blocking`.
- **Rationale**: Atomics keep the hot path lock-free; `Permit` (RAII) guarantees release on drop, which is what makes drain and reload safe. Policies are declared per buffer by its owner, not by the administrator, matching FR-038.
- **Alternatives considered**: `tokio::sync::Semaphore` per buffer (rejected: permits are `u32` and cannot be shrunk below outstanding count, which FR-040 requires); a single global memory limiter (rejected: spec wants named buffers).

## R10. Reload orchestration (FR-012–FR-015, FR-037)

- **Decision**: `node::reload::reload(path) -> Result<ReloadReport, Vec<ConfigError>>`: parse+validate the file with the same `config` code → diff against the running `EffectiveConfig` using `reload_class::diff()` → if any error, return them and change nothing → apply live settings (buffer capacities via `Buffer::set_capacity`, drain timeout via `lifecycle`) → swap `Arc<EffectiveConfig>` with `pending_restart` markers for restart-required diffs → return report `{ changed, applied_live, pending_restart }`. Serialized with a `tokio::sync::Mutex` so concurrent reloads apply in order. Rejected with `node_state` error when state ∉ {`ready`}. Triggers: `POST /v1/reload`, `admin` op `reload`, CLI `spacestorage reload`. `SIGHUP` is *not* mapped in v1 (documented alternative).
- **Rationale**: Reusing `config` guarantees identical validation offline and online. Arc-swap makes readers lock-free.
- **Alternatives considered**: Watching the file with inotify (rejected: implicit, surprising); `SIGHUP` (deferred: conventional, trivial to add, but the spec named API + CLI).

## R11. Lifecycle and graceful stop (FR-005–FR-007)

- **Decision**: `NodeState` = `Starting | Ready | Draining | Failed` (names match `08`'s `node_state` vocabulary; `degraded`/`recovering` reserved for later features). `SIGTERM`/`SIGINT` or admin `stop` → `Draining`: stop accept loops (drop listeners), send `CancellationToken` to handlers so they finish current requests, wait for the in-flight task tracker (`tokio_util::task::TaskTracker`) up to `drain_timeout` (default 30 s), then abort remaining tasks, record `drain_timed_out=true` in stats/log, exit 0. Second signal during `Draining` → immediate exit. Signal before `Ready` → unbind everything and exit without entering `Ready` (exit code 0, log "startup aborted").
- **Rationale**: Matches Story 1 scenarios 5–6 and edge cases exactly; `TaskTracker` gives a precise in-flight count for the stats module.
- **Alternatives considered**: Per-handler custom drain hooks (deferred: the token+tracker pair is the contract handlers implement; specialized behaviour can be layered by `02`).

## R12. CLI framework, output, exit codes (FR-031–FR-037)

- **Decision**: `clap` 4 derive; global flags `--endpoint <host:port>`, `--via admin|admin-http` (default `admin-http`), `--token-file <path>`, `--ca <path>`, `--timeout <secs>` (default 5), `--output human|json` (default `human`). Commands: `validate <config>`, `status`, `config`, `threads`, `buffers`, `reload`, `stop [--wait]`. Human output via a small table renderer (no `comfy-table` dependency needed); JSON output is the raw `admin-proto` types. Exit codes: 0 ok, 2 validation failure, 3 connection failure, 4 node-reported error, 5 reload applied with restart pending, 1 usage error (clap default).
- **Rationale**: One shared type set gives FR-035 stable field names for free; exit-code table satisfies FR-036.
- **Alternatives considered**: `argh`/`pico-args` (rejected: less ergonomic for subcommands and help); TUI (out of scope, `09`).

## R13. Logging and stats

- **Decision**: `tracing` with `tracing-subscriber` `fmt` layer (text on TTY, JSON when `log { format json; }`), level from `log { level info; }`. `stats.rs` keeps atomics/gauges with names reserved for `08`: `spacestorage_node_state`, `spacestorage_node_ready`, `spacestorage_uptime_seconds`, `spacestorage_worker_threads`, `spacestorage_worker_threads_busy`, `spacestorage_buffer_capacity_bytes{buffer}`, `spacestorage_buffer_usage_bytes{buffer}`, `spacestorage_buffer_usage_ratio{buffer}`, `spacestorage_buffer_limit_hits_total{buffer}`, `spacestorage_entrypoint_connections_active{entrypoint,handler}`. No exposition crate is added in this feature.
- **Rationale**: Principle X requires in-memory stats now and Prometheus naming later; reserving names avoids a rename when `08` lands.
- **Alternatives considered**: Adding the `prometheus` crate and `/metrics` now (rejected: owned by `08`; the Observability Contract says plans may add series but the catalog is `08`'s).

## R14. Testing strategy

- **Decision**: Unit tests in each crate; `crates/node/tests/*` boot the node in-process using `port 0` ephemeral binding exposed via a test-only `bound_addrs()` hook and drive both admin handlers; fixture-driven contract tests read every file in `specs/001-runtime-cli-api/contracts/fixtures/` and assert on the error list (`invalid/*.conf` each carry an `# expect: <setting>` comment header); CLI tests spawn the built binary. SC-003 latency test holds one `admin` connection in a slow `hello` while timing 200 `status` calls.
- **Rationale**: Every SC in the spec maps to at least one test file (see plan Project Structure).
- **Alternatives considered**: Docker-based e2e (deferred: not needed for single-node scope).

## Resolved unknowns summary

| Unknown | Resolution |
|---------|-----------|
| Config format | nginx-style blocks, hand-written parser (R1) |
| Default listen address | `127.0.0.1` (R7) |
| Certificate reference forms | file paths in v1; `vault:` reserved (R6) |
| Drain timeout default | 30 s (R11) |
| Interim admin auth | `admin { token_file }` bearer token (R8) |
| Busy-thread measurement | Tokio `RuntimeMetrics` with in-flight fallback (R3) |
| Buffer inventory & policies | `net.recv`/`net.send` Wait, `request.queue` Reject (R9) |
| TCP admin framing | u32 length + JSON, `hello` handshake (R4) |
