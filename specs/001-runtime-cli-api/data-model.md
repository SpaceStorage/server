# Data Model: SpaceStorage Runtime, CLI, and Node Interfaces

**Feature**: `001-runtime-cli-api` | **Date**: 2026-09-13 | **Source**: [spec.md](spec.md) Key Entities, [research.md](research.md)

Nothing in this feature is persisted. All entities live in process memory or in the administrator's configuration file. Types below are described in Rust-ish notation for precision; crate ownership is noted.

## 1. Configuration model (`crates/config`)

### `NodeConfig`

Typed result of resolving the configuration file plus launch overrides. Produced by `resolve()`, checked by `validate()`.

| Field | Type | Default | Reload class | Validation |
|-------|------|---------|--------------|------------|
| `node.name` | `String` | hostname | restart | non-empty, ≤ 253 chars |
| `runtime.threads` | `Option<u32>` | `None` → available cores | restart | if set: ≥ 1 |
| `runtime.drain_timeout` | `Duration` | 30 s | **live** | 1 s – 24 h |
| `log.level` | `Level` | `info` | live | one of `error|warn|info|debug|trace` |
| `log.format` | `LogFormat` | `text` | restart | `text|json` |
| `admin.token_file` | `Option<PathBuf>` | `None` | live (re-read) | required if any admin handler enabled; file readable, non-empty after trim |
| `admin_handlers.admin` | `AdminHandlerDecl` | — | restart | **mandatory declaration** |
| `admin_handlers.admin_http` | `AdminHandlerDecl` | — | restart | **mandatory declaration** |
| `entrypoints` | `Vec<EntrypointDecl>` | `[]` | restart | see below; `(address, port)` unique |
| `buffers` | `BTreeMap<String, u64>` | `{}` | **live** | name in registry; value within that buffer's range |

### `AdminHandlerDecl`

```text
enum AdminHandlerDecl { Enabled /* ≥1 entrypoint with this handler exists */, Disabled /* explicit `admin disabled;` */ }
```

Derived: `Enabled` when at least one `entrypoint` declares `handler admin;` (resp. `admin-http`); `Disabled` when the top-level directive `disable admin;` / `disable admin-http;` is present. Both absent → error `admin_handler_undeclared{handler}`. Both present → error `admin_handler_conflict{handler}`.

### `EntrypointDecl`

| Field | Type | Default | Validation |
|-------|------|---------|------------|
| `name` | `String` | `"<handler>@<address>:<port>"` | unique if given |
| `address` | `IpAddr` | `127.0.0.1` | parseable IPv4/IPv6 |
| `port` | `u16` | — (mandatory) | 1–65535 |
| `handler` | `String` | — (mandatory, exactly one) | in `HandlerRegistry` |
| `tls` | `Option<TlsDecl>` | `None` | see below |

Errors: `entrypoint_missing_port`, `entrypoint_missing_handler`, `entrypoint_multiple_handlers`, `entrypoint_unknown_handler{handler, known: [..]}`, `entrypoint_duplicate_address{other}`.

### `TlsDecl`

| Field | Type | Validation |
|-------|------|------------|
| `certificate` | `CertRef` | `CertRef::File(path)` in v1; path readable; PEM parses to ≥ 1 cert; leaf `not_before ≤ now ≤ not_after` |
| `key` | `CertRef` | file readable; PEM private key parses |
| `client_ca` | `Option<CertRef>` | reserved (mTLS in `07`); error `unsupported_in_this_version` if set |

`CertRef` grammar: bare path or `file:<path>`; `vault:<path>` parses but yields `cert_ref_scheme_unsupported` in this feature. Inline PEM (`-----BEGIN`) yields `cert_inline_forbidden`.

### `ConfigError`

```text
struct ConfigError { file: PathBuf, line: u32, col: u32, setting: String /* dotted path, e.g. entrypoint[2].handler */, code: ErrorCode, message: String }
```

`validate()` returns `Vec<ConfigError>`; never stops at the first.

### `ReloadClass` and `ConfigDiff`

```text
enum ReloadClass { Live, RestartRequired }
struct SettingChange { setting: String, from: Value, to: Value, class: ReloadClass }
struct ConfigDiff { changes: Vec<SettingChange> }
```

`diff(running: &NodeConfig, incoming: &NodeConfig) -> ConfigDiff`. Entrypoint and thread changes are `RestartRequired`; buffer capacities, drain timeout, log level, token file content are `Live`.

## 2. Runtime model (`crates/node`)

### `Node`

| Field | Type | Notes |
|-------|------|-------|
| `name` | `String` | from config |
| `state` | `watch::Sender<NodeState>` | see state machine |
| `started_at` | `Instant` | uptime = now − started_at |
| `effective` | `ArcSwap<EffectiveConfig>` | lock-free snapshot for admin reads |
| `entrypoints` | `Vec<BoundEntrypoint>` | bound listeners + handler + TLS acceptor |
| `handlers` | `HandlerRegistry` | inventory |
| `buffers` | `BufferRegistry` | inventory |
| `stats` | `Arc<Stats>` | atomics |
| `tasks` | `TaskTracker` | in-flight work for drain |
| `cancel` | `CancellationToken` | drain signal to handlers |
| `reload_lock` | `Mutex<()>` | serializes reloads |

### `NodeState` (state machine, FR-005–FR-007, FR-015)

```text
Starting ──(all listeners bound, TLS loaded, token loaded)──▶ Ready
Starting ──(stop signal | bind error | validation error)────▶ Failed*  (*startup abort exits 0 on signal, non-zero on error)
Ready    ──(SIGTERM | SIGINT | admin stop)──────────────────▶ Draining
Draining ──(in-flight == 0 | drain_timeout elapsed)────────▶ (process exit 0)
Draining ──(second signal)─────────────────────────────────▶ (process exit 0, immediate)
```

Reload allowed only in `Ready`. Health: `/v1/health/live` → 200 in any state but exit; `/v1/health/ready` → 200 only in `Ready`.

### `Handler` trait and `HandlerRegistry`

```text
trait Handler: Send + Sync {
    fn name(&self) -> &'static str;               // "admin", "admin-http", "cassandra", …
    fn kind(&self) -> HandlerKind;                // Administrative | ClientFacing | Internal
    fn owner(&self) -> &'static str;              // feature id, e.g. "01-runtime-cli-api"
    async fn serve(&self, conn: Conn, ctx: ConnCtx) -> Result<(), HandlerError>;
}
struct HandlerRegistry { by_name: BTreeMap<&'static str, Arc<dyn Handler>> }
```

Built-in in this feature: `admin` (Administrative), `admin-http` (Administrative). `register(handler)` fails on duplicate name. The inventory (`name`, `kind`, `owner`) is reported in `EffectiveConfig.handlers`.

### `BoundEntrypoint`

| Field | Type |
|-------|------|
| `decl` | `EntrypointDecl` |
| `local_addr` | `SocketAddr` (actual, matters for port 0 in tests) |
| `handler` | `Arc<dyn Handler>` |
| `tls` | `Option<TlsAcceptor>` |
| `transport` | `Transport::{Encrypted, Plaintext}` |
| `cert_expired` | `AtomicBool` (runtime expiry marker) |
| `connections_active` | `AtomicU64` |

Invariant: all entrypoints are bound before `state` becomes `Ready`; on any bind failure all already-bound listeners are dropped (FR-007, FR-023).

### `Buffer` and `BufferRegistry` (FR-038–FR-044)

```text
enum OverflowPolicy { Reject, Wait }
struct BufferSpec { name: &'static str, default: u64, range: RangeInclusive<u64>, policy: OverflowPolicy, owner: &'static str }
struct Buffer { spec: BufferSpec, capacity: AtomicU64, used: AtomicU64, limit_hits: AtomicU64, notify: Notify }
struct Permit<'a> { buf: &'a Buffer, n: u64 }   // releases on drop
```

Operations:

- `try_reserve(n) -> Result<Permit, Overflow>` — if `used + n > capacity`: `limit_hits += 1`, return `Overflow` (Reject) or await `notify` then retry (Wait).
- `release(n)` — `used -= n`, `notify.notify_waiters()`.
- `set_capacity(c)` — atomic store; allowed below `used` (FR-040); readers compute `ratio = used / capacity` which may exceed 1.0.
- `report() -> BufferReport { name, capacity, used, ratio, limit_hits, policy, range, default, owner }`.

Built-ins (registered by this feature):

| Name | Default | Range | Policy |
|------|---------|-------|--------|
| `net.recv` | 64 MiB | 1 MiB – 64 GiB | Wait |
| `net.send` | 64 MiB | 1 MiB – 64 GiB | Wait |
| `request.queue` | 16 MiB | 1 MiB – 16 GiB | Reject |

Registry invariant: buffer names referenced in `buffers { … }` must exist at validate time; the CLI's offline `validate` uses the same built-in list plus a compiled-in list of feature buffers.

### `Stats`

Atomics named per [research R13](research.md#r13-logging-and-stats). Includes `drain_timed_out: AtomicBool`, `worker_threads: u32`, `worker_threads_busy: AtomicU32`, `uptime_seconds` (derived).

### `EffectiveConfig` (FR-010, FR-012, FR-014, FR-029)

Serializable snapshot (`admin-proto::EffectiveConfig`):

```text
struct EffectiveConfig {
  node: { name },
  runtime: { threads: u32, threads_source: "configured"|"available_cores", drain_timeout_secs: u64 },
  log: { level, format },
  admin: { token_file: Option<String>, auth: "token_file" },
  admin_handlers: { admin: "enabled"|"disabled", admin_http: "enabled"|"disabled" },
  entrypoints: [ { name, address, port, handler, transport: "encrypted"|"plaintext", cert_expired: bool, connections_active: u64 } ],
  handlers: [ { name, kind, owner } ],
  buffers: [ BufferReport ],
  settings: [ { setting, value, reload_class: "live"|"restart_required", pending_restart: Option<Value> } ],
  config_path: String, loaded_at: RFC3339, reloaded_at: Option<RFC3339>
}
```

`pending_restart` is set on a setting when the last reload's diff had a `RestartRequired` change for it; cleared only by restart.

### `ReloadReport`

```text
struct ReloadReport { ok: bool, changed: Vec<SettingChange>, applied_live: Vec<String>, pending_restart: Vec<String>, warnings: Vec<String> /* e.g. buffers exceed memory */, errors: Vec<ConfigError> }
```

## 3. Admin protocol model (`crates/admin-proto`)

```text
enum AdminOp { Status, Config, Threads, Buffers, Reload, Stop { wait: bool } }
struct Status { node_name, state, uptime_seconds, threads: ThreadsReport, entrypoints: [...], buffers: [BufferReport], version }
struct ThreadsReport { total: u32, busy: u32, source }
struct ErrorBody { code: ErrorCode, message: String, details: Value }
enum ErrorCode { Unauthorized, InvalidState { state }, ConfigInvalid, Internal, UnknownOp, ProtocolVersion }
// TCP framing
struct Frame { len: u32 /* BE */, payload: Vec<u8> /* JSON */ }
struct Hello { proto_version: u16, token: String, client: String }
struct Request { id: u64, op: String, args: Value }
struct Response { id: u64, ok: bool, result: Option<Value>, error: Option<ErrorBody> }
```

## 4. CLI model (`crates/spacestorage`)

```text
struct Global { endpoint: SocketAddr, via: Via::{Admin, AdminHttp}, token_file: Option<PathBuf>, ca: Option<PathBuf>, timeout: Duration, output: Output::{Human, Json} }
enum Command { Validate { config: PathBuf }, Status, Config, Threads, Buffers, Reload, Stop { wait: bool } }
enum ExitCode { Ok = 0, Usage = 1, ValidationFailed = 2, ConnectionFailed = 3, NodeError = 4, RestartPending = 5 }
```

## 5. Relationships

```text
NodeConfig 1 ──▶ * EntrypointDecl ──1▶ Handler (by name, from HandlerRegistry)
NodeConfig 1 ──▶ * (buffer name → capacity) ──▶ Buffer (from BufferRegistry)
Node 1 ──▶ * BoundEntrypoint ──▶ 1 Handler, 0..1 TlsAcceptor
Node 1 ──▶ 1 EffectiveConfig (ArcSwap) ◀── ReloadReport updates
Handler * ──▶ * Buffer (via Permit reservations)
CLI ──(admin | admin-http)──▶ AdminService ──▶ Node
```
