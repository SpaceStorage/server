# Data Model: Wire Protocols and Datatype-Aware Drivers

**Feature**: `002-protocol-drivers` | **Date**: 2026-09-13 | **Source**: [spec.md](spec.md) Key Entities, [research.md](research.md)

Persistence in this feature is in-memory only (interim type inventory, R16); durable storage arrives with `03`. Types in Rust-ish notation; crate ownership noted. Entities from `001` (`Node`, `EntrypointDecl`, `Handler`, `HandlerRegistry`, `BufferRegistry`, `EffectiveConfig`) are reused, not redefined.

## 1. Configuration additions (`crates/config`)

### `QueryDefaults` — `query_defaults { … }`

| Field | Type | Default | Reload | Validation |
|-------|------|---------|--------|------------|
| `write_quorum` | `QuorumLevel` | `TWO` | **live** | in vocabulary (§4) |
| `read_quorum` | `QuorumLevel` | `ONE` | **live** | in vocabulary |
| `timeout` | `Duration` | `30s` | **live** | `1ms`–`24h`, > 0 |
| `max_timeout` | `Option<Duration>` | `10m` | **live** | ≥ `timeout` |

Errors: `query_defaults_bad_quorum{setting, value, vocabulary}`, `query_defaults_timeout_out_of_range`, `query_defaults_max_below_default`. Provenance reported as `configured | built_in`.

### `AuthConfig` — `auth { users_file P; }` (interim, R11)

| Field | Type | Reload | Validation |
|-------|------|--------|------------|
| `users_file` | `PathBuf` | live (re-read) | required if any client-facing handler is declared; readable; mode ≤ `0600` on Unix (`users_file_permissions`); every line parses (`users_file_syntax{line}`); user names unique (`users_file_duplicate_user`) |

Users file line: `name secret namespace [schema=public] [roles=a,b]`.

### Handler inventory additions

`HandlerRegistry` gains eight `ClientFacing` handlers owned by `02-protocol-drivers`: `postgresql`, `cassandra`, `redis`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, `webdav`. Validation of `entrypoint.handler` is unchanged; FR-003 (two *different* protocol handlers on one `address:port`) is already covered by `entrypoint_duplicate_address`.

## 2. Identity and addressing (`crates/types`)

### `Namespace`, `Schema`, `ContainerRef`

```text
struct NamespaceName(String);   // 1–63 chars, [a-z0-9_-], unique cluster-wide
struct SchemaName(String);      // default "public"
struct ContainerRef { namespace: NamespaceName, schema: SchemaName, name: ContainerName }
struct ContainerName(String);   // 1–255 chars, any UTF-8 except '/' and NUL; uniqueness per (namespace, schema)
```

Lifecycle: a namespace is created implicitly on first successful `CreateContainer` or first write to the implicit Redis container (interim, until `07` adds namespace administration).

### `Principal` (`crates/auth`)

| Field | Type | Notes |
|-------|------|-------|
| `name` | `String` | user / access key id |
| `namespace` | `Option<NamespaceName>` | bound namespace; `None` allowed only for principals with role `admin` on protocols with native selection (FR-009a) |
| `schema` | `SchemaName` | default `public` |
| `roles` | `Vec<String>` | interim: `admin` grants any-namespace selection; everything else is opaque until `07` |

### `Credential` (input to `Authenticator`)

```text
enum Credential {
  Password { user, secret },                         // redis AUTH, CQL SASL PLAIN, CH password, Basic
  ScramSha256 { user, client_first, client_final },  // postgresql (pgwire drives the exchange)
  SigV4 { access_key, scope, string_to_sign, signature },   // s3
  DigestMd5 { user, realm, nonce, uri, response, … }, // webdav
}
```

## 3. Type system seam (`crates/types`)

### `TypeRegistry` and `Datatype`

```text
struct TypeName(&'static str);   // "kv_collection", "relational_table", "document_store", "object_collection", "vector_collection", "ordered_map"
enum Level { L0, L1, L2, L3, L4 }
struct OperationSpec { name: &'static str, args: CanonicalSchema, result: CanonicalSchema, mutates: bool }
struct CreateOption { name: &'static str, kind: OptionKind, required: bool, default: Option<CanonicalValue> }

trait Datatype: Send + Sync {
  fn name(&self) -> TypeName;
  fn level(&self) -> Level;
  fn create_options(&self) -> &[CreateOption];
  fn validate_create_options(&self, opts: &BTreeMap<String, CanonicalValue>) -> Result<(), TypeError>;
  fn open(&self, meta: &ContainerMeta) -> Result<Arc<dyn Container>, TypeError>;
  fn operations(&self) -> &[OperationSpec];
  fn canonical_schema(&self) -> CanonicalSchema;     // shape of "$d" for values of this type
}
struct TypeRegistry { by_name: BTreeMap<TypeName, Arc<dyn Datatype>> }
```

Invariant (FR-018): at startup, for every declared client-facing handler `h` and every `t ∈ TypeRegistry`, `h.mapping(t)` is `Native(…)` or `Canonical(carrier)`; the check is structural (every driver declares a carrier), so it can only fail if a driver forgets to declare its carrier — reported as `driver_mapping_missing{handler, type}`.

### `ContainerMeta` and `Container`

| Field | Type | Notes |
|-------|------|-------|
| `ref` | `ContainerRef` | |
| `type_name` | `TypeName` | |
| `options` | `BTreeMap<String, CanonicalValue>` | validated create options |
| `created_at`, `altered_at` | `DateTime<Utc>` | |
| `created_via` | `ProtocolName` | informational (Story 2 scenario 10) |
| `stats` | `ContainerStats` | items, bytes, ops counters (for `08`) |

```text
trait Container: Send + Sync {
  fn meta(&self) -> &ContainerMeta;
  async fn get(&self, key: &Key) -> Result<Option<CanonicalValue>>;
  async fn put(&self, key: Key, value: CanonicalValue, cond: PutCond) -> Result<PutOutcome>;
  async fn delete(&self, key: &Key) -> Result<bool>;
  async fn exists(&self, key: &Key) -> Result<bool>;
  fn scan(&self, q: ScanQuery) -> BoxStream<Result<Row>>;
  async fn aggregate(&self, q: AggQuery) -> Result<Vec<Row>>;
  async fn type_op(&self, op: &str, args: CanonicalValue) -> Result<CanonicalValue>;   // registry-declared ops (e.g. "knn")
  fn object(&self) -> Option<&dyn ObjectOps>;          // Some for byte-stream types
  async fn alter(&self, opts: BTreeMap<String, CanonicalValue>) -> Result<()>;
}
trait ObjectOps { put_stream, get_range, head, copy, list_prefix{delimiter}, multipart_begin/part/complete/abort }
```

### `CanonicalValue` and `CanonicalSchema` (R9)

```text
enum CanonicalValue { Null, Bool, Int(i64), UInt(u64), Float(f64), Str, Bytes(Vec<u8>) /* "$b" */, Timestamp /* "$t" */, Array(Vec<_>), Object(BTreeMap<String, _>) }
struct Envelope { r#type: String /* "<type>/<kind>" */, v: u16, d: CanonicalValue }
fn to_canonical_bytes(&Envelope) -> Vec<u8>   // deterministic (sorted keys, shortest floats)
fn from_canonical_bytes(&[u8]) -> Result<Envelope, CanonicalError { position, reason }>
```

Rules: keys sorted by UTF-16 code units; no whitespace; integers exact; floats shortest-round-trip; `NaN`/`±Infinity` as strings; bytes base64url no padding; media type `application/vnd.spacestorage.canonical+json; v=1`. Byte-identical for equal logical values (Story 2 scenario 4a).

### Interim inventory (this feature)

| Type | Level | Create options | Key | Native default for | Type ops |
|------|-------|----------------|-----|--------------------|----------|
| `kv_collection` | L2 | — | string | Redis implicit `redis` container | `incr`, `expire`, `hset/hget` (hash values) |
| `relational_table` | L3 | `columns` (name:type list), `primary_key` | PK tuple | PG/CQL/CH tables | — |
| `document_store` | L3 | `id_field` (default `_id`) | doc id | ES index | `mapping` (derived) |
| `object_collection` | L2 | `versioning=false` | object key | S3 bucket, WebDAV collection | `multipart_*`, `list_prefix` |
| `vector_collection` | L2 | `dims` (required), `metric ∈ cosine\|l2\|dot` | item id | — (foreign everywhere) | `knn { vector, k, filter? } -> [ {id, score, payload} ]` |
| `ordered_map` | L2 | — | string (ordered) | — (system: WebDAV locks `.webdav_locks`) | `range { from, to, limit }` |

## 4. Query options (`crates/exec`)

```text
enum QuorumLevel { One, Two, Three, Quorum, LocalQuorum, EachQuorum, LocalOne, All, Acks(u16) }
enum OptionSource { Query, Session, NamespaceDefault, GlobalDefault }
enum ClampReason { ReplicaCount { requested: QuorumLevel, available: u16 }, MaxTimeout { requested: Duration, max: Duration } }
struct Sourced<T> { value: T, source: OptionSource, clamped: Option<ClampReason> }
struct QueryOptions {
  quorum: Sourced<QuorumLevel>, timeout: Sourced<Duration>,
  namespace: NamespaceName, schema: SchemaName, principal: Arc<Principal>,
  protocol: ProtocolName, protocol_version: String, client_app: Option<String>,
}
struct SessionOptions { quorum: Option<QuorumLevel>, timeout: Option<Duration> }   // per Client Session
```

Resolution (FR-025): `query.quorum ?? session.quorum ?? namespace_default ?? global.(write|read)_quorum` with `source` set accordingly; same for `timeout` (then clamp to `max_timeout`). Read vs write default chosen by `LogicalRequest::is_mutation()`. Clamping (FR-030): if `!placement.satisfiable(quorum, container)` → `source ∈ {Query, Session}` ⇒ `ExecError::QuorumUnsatisfiable{requested, available}`; else clamp to `Acks(available)`.

Validation of client-supplied values (FR-031): quorum parse failure → `InvalidOptionValue{option:"quorum", value}`; timeout ≤ 0 or unparseable → `InvalidOptionValue{option:"timeout"}`; session unchanged on error.

## 5. Sessions (`crates/protocol-core`)

### `ClientSession`

| Field | Type | Notes |
|-------|------|-------|
| `id` | `u64` | node-unique |
| `protocol` | `ProtocolName` | handler name |
| `protocol_version` | `String` | negotiated (`"3.0"`, `"v4"`, `"RESP2"`, `"HTTP/1.1"`, `"54460"`) |
| `entrypoint` | `EntrypointName` | |
| `transport` | `Encrypted \| Plaintext` | from `BoundEntrypoint` |
| `principal` | `Arc<Principal>` | after auth |
| `namespace`, `schema` | | bound per R11/R12 |
| `namespace_source` | `ClientSelected \| CredentialBound` | |
| `options` | `SessionOptions` | |
| `target_container` | `Option<ContainerName>` | Redis `SS.USE` only |
| `client_app` | `Option<String>` | PG `application_name`, CQL `APPLICATION_NAME`, Redis `CLIENT SETNAME`, HTTP `User-Agent` |
| `started_at` | `Instant` | |
| `in_flight` | `AtomicU32` | for drain |

State machine (FR-040):

```text
Connecting ──(signature ok ≤1s)──▶ Handshaking ──(auth ok, namespace bound)──▶ Ready ──(request)──▶ Executing ──▶ Ready
Connecting ──(mismatch | 1s deadline)──▶ Closed(reason=mismatch)
Handshaking ──(auth fail | no namespace)──▶ Closed(reason=auth)
Ready/Executing ──(node Draining: finish in-flight, refuse new)──▶ Closed(reason=drain)
```

For stateless HTTP protocols a `ClientSession` is one keep-alive TCP connection; `X-SpaceStorage-Session` mutates `options` for the remaining requests on that connection.

## 6. Execution boundary (`crates/exec`)

```text
enum LogicalRequest { Ddl(Ddl), Point(Point), Scan(Scan), Aggregate(Aggregate), Join(Join), Mutate(Mutate), TypeOp(TypeOp), Object(ObjectReq), Batch(Vec<LogicalRequest>) }
// see contracts/execution-boundary.md for each variant's fields and Expr
trait QueryEngine: Send + Sync {
  fn execute(&self, req: LogicalRequest, opts: QueryOptions) -> BoxStream<'static, ExecutionEvent>;
}
enum ExecutionEvent { Schema(RowSchema), Rows(RowBatch), Progress { rows, bytes }, Done(ExecStats), Error(ExecError) }
enum ExecError { NotSupported { what }, QuorumUnsatisfiable { requested, available }, Timeout { applied }, InvalidOptionValue { option, value }, TypeMismatch { container_type, field, value }, NotFound { what }, AlreadyExists { what }, Unauthorized, Forbidden { namespace }, Internal }
trait PlacementInfo { fn replicas(&self, c: &ContainerRef) -> u16; fn satisfiable(&self, q: QuorumLevel, c: &ContainerRef) -> bool; }  // LocalEngine: 1
```

### `ExecutionRecord` (FR-037, test hook)

| Field | Type |
|-------|------|
| `id` | `u64` |
| `protocol`, `protocol_version` | |
| `principal`, `namespace`, `schema` | |
| `kind` | `RequestKind` (ddl/get/put/delete/scan/aggregate/join/type_op/object/batch) |
| `container` | `Option<ContainerRef>` |
| `applied_quorum` | `Sourced<QuorumLevel>` |
| `applied_timeout` | `Sourced<Duration>` |
| `started_at`, `duration` | |
| `outcome` | `Ok { rows, bytes } \| Error { error_type }` |

Stored in a bounded ring (default 10 000, buffer `exec.records` registered in `BufferRegistry`) and exposed via admin op `executions` (`GET /v1/executions?protocol=&limit=`) — administrative, `admin` token protected.

## 7. Driver-side mapping model (`crates/protocol-core`)

```text
enum Mapping { Native(NativeMapping), Canonical(Carrier) }
struct NativeMapping { read: &'static str /* description */, write_forms: &'static [&'static str], ops: &'static [(&'static str /* type op */, &'static str /* protocol surface */)] }
enum Carrier { Text, Bytes, Json, Body { media_type: &'static str } }
trait ProtocolDriver {   // implemented once per protocol family; the Handler adapts transport
  fn protocol(&self) -> ProtocolName;
  fn mapping(&self, t: TypeName) -> Mapping;                       // FR-014, FR-018
  fn default_type_for_create(&self, verb: CreateVerb) -> Option<TypeName>;   // FR-014a
  fn escape_name(&self, n: &ContainerName) -> String; fn unescape_name(&self, s: &str) -> Result<ContainerName>;   // FR-019
}
```

## 8. Per-protocol error form (`ErrorRenderer`)

| Protocol | `NotSupported` | `Timeout` | `QuorumUnsatisfiable` | `InvalidOptionValue` | Auth failure | Protocol mismatch |
|----------|----------------|-----------|-----------------------|----------------------|--------------|-------------------|
| postgresql | `ErrorResponse` `0A000` feature_not_supported | `57014` query_canceled, message "statement timeout (applied 5s, source session)" | `55000` object_not_in_prerequisite_state | `22023` invalid_parameter_value | `28P01` | `08P01` |
| cassandra | `ERROR` `INVALID (0x2200)` | `READ_TIMEOUT`/`WRITE_TIMEOUT` (0x1200/0x1100) with consistency | `UNAVAILABLE (0x1000)` {cl, required, alive} | `INVALID` | `AUTH_ERROR (0x0100)` | `PROTOCOL_ERROR (0x000A)` |
| redis | `-ERR unsupported: <what>` | `-TIMEOUT applied=5000ms source=session` | `-UNAVAILABLE requested=THREE available=2` | `-ERR invalid option value` | `-WRONGPASS` / `-NOAUTH` | `-ERR unknown protocol` |
| elasticsearch | `501` `not_implemented_exception` | `504` `timeout_exception` | `503` `unavailable_shards_exception` | `400` `illegal_argument_exception` | `401` `security_exception` | `400` text/plain |
| clickhouse (native) | `Exception` code 48 `NOT_IMPLEMENTED` | code 159 `TIMEOUT_EXCEEDED` | code 285 `TOO_FEW_LIVE_REPLICAS` | code 36 `BAD_ARGUMENTS` | code 516 `AUTHENTICATION_FAILED` | code 101 `UNEXPECTED_PACKET_FROM_CLIENT` |
| clickhouse-http | `501` + `X-ClickHouse-Exception-Code: 48` | `504` + code 159 | `503` + code 285 | `400` + code 36 | `403` + code 516 (CH convention) | `400` |
| s3 | `501 NotImplemented` | `504` `<Code>RequestTimeout</Code>` | `503 ServiceUnavailable` `<Code>QuorumUnsatisfiable</Code>` (extension) | `400 InvalidArgument` | `403 SignatureDoesNotMatch` / `InvalidAccessKeyId` | `400` |
| webdav | `501 Not Implemented` | `504 Gateway Timeout` + `X-SpaceStorage-Error: timeout` | `503` + `X-SpaceStorage-Error: quorum_unsatisfiable` | `400` | `401` + `WWW-Authenticate: Basic, Digest` | `400` |

Every rendered error carries the applied option values where relevant (FR-029) via message text (SQL/CQL/RESP) or `X-SpaceStorage-Applied-*` headers (HTTP).

## 9. Relationships

```text
NodeConfig 1 ──▶ 1 QueryDefaults, 1 AuthConfig, * EntrypointDecl ──▶ Handler(ClientFacing) ──▶ 1 ProtocolDriver
ProtocolDriver * ──▶ * Mapping (one per TypeName in TypeRegistry)
Handler::serve ──▶ 1 ClientSession ──▶ 1 Principal ──▶ 0..1 NamespaceName
ClientSession ──(per request)──▶ QueryOptions + LogicalRequest ──▶ QueryEngine ──▶ TypeRegistry ──▶ Container
QueryEngine ──▶ * ExecutionRecord (ring, admin `executions`)
QueryEngine ──▶ PlacementInfo (LocalEngine: replicas = 1; `04`/`06` later)
Container(ordered_map ".webdav_locks") ◀── webdav LOCK/UNLOCK
```
