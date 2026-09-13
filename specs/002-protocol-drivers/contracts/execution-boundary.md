# Contract: Execution Boundary (`LogicalRequest`, `QueryOptions`, `QueryEngine`)

**Feature**: `002-protocol-drivers` (consumer; interim `LocalEngine` provider) | Crate: `crates/exec` (`spacestorage-exec`) | Future owner: feature `05-query-execution`

Every driver lowers its wire request into a `LogicalRequest`, attaches `QueryOptions`, and calls the single `QueryEngine` (spec FR-035). No driver executes data operations by any other path.

## `QueryEngine`

```rust
pub trait QueryEngine: Send + Sync {
    fn execute(&self, req: LogicalRequest, opts: QueryOptions) -> BoxStream<'static, ExecutionEvent>;
    fn placement(&self) -> &dyn PlacementInfo;
}
pub enum ExecutionEvent { Schema(RowSchema), Rows(RowBatch), Progress { rows: u64, bytes: u64 }, Done(ExecStats), Error(ExecError) }
pub struct ExecStats { rows: u64, bytes: u64, elapsed: Duration, applied: AppliedOptions }
```

Guarantees the engine gives drivers:

1. Exactly one terminal event (`Done` or `Error`).
2. `Error(NotSupported)` and `Error(QuorumUnsatisfiable)` are emitted **before** any mutation (FR-030, FR-038).
3. Timeout: the engine cancels work and emits `Error(Timeout{applied})` no later than `applied + 1s` (FR-029).
4. Every call produces an `ExecutionRecord` (FR-037) with `protocol`, `principal`, `namespace`, `kind`, `applied` options and outcome.
5. Default-sourced quorum that is unsatisfiable is clamped (recorded in `applied.quorum.clamped`); explicit is rejected (Clarification Q1).

## `QueryOptions`

```rust
pub struct QueryOptions {
    pub quorum: Sourced<QuorumLevel>,      // resolved by protocol-core::resolve_options (see query-options.md)
    pub timeout: Sourced<Duration>,
    pub namespace: NamespaceName, pub schema: SchemaName,
    pub principal: Arc<Principal>,
    pub protocol: ProtocolName, pub protocol_version: String, pub client_app: Option<String>,
}
```

## `LogicalRequest`

```rust
pub enum LogicalRequest {
    Ddl(Ddl), Point(Point), Scan(Scan), Aggregate(Aggregate), Join(Join), Mutate(Mutate), TypeOp(TypeOp), Object(ObjectReq),
    Batch(Vec<LogicalRequest>),            // executed in order; atomicity per element until `05` adds transactions
}
pub enum Ddl {
    CreateContainer { r#ref: ContainerRef, type_name: TypeName, options: Options, if_not_exists: bool },
    AlterContainer  { r#ref: ContainerRef, options: Options },
    DropContainer   { r#ref: ContainerRef, if_exists: bool },
    ListContainers  { namespace: NamespaceName, schema: Option<SchemaName> },      // returns rows {schema, name, type, created_via, created_at, options}
    DescribeContainer { r#ref: ContainerRef },
}
pub enum Point {
    Get { c: ContainerRef, key: Key }, Put { c: ContainerRef, key: Key, value: CanonicalValue, cond: PutCond },
    Delete { c: ContainerRef, key: Key }, Exists { c: ContainerRef, key: Key },
    MultiGet { c: ContainerRef, keys: Vec<Key> }, MultiPut { c: ContainerRef, entries: Vec<(Key, CanonicalValue)> },
}
pub struct Scan      { pub c: ContainerRef, pub projection: Option<Vec<FieldPath>>, pub filter: Option<Expr>, pub sort: Vec<(FieldPath, Order)>, pub limit: Option<u64>, pub offset: u64 }
pub struct Aggregate { pub c: ContainerRef, pub group_by: Vec<FieldPath>, pub aggs: Vec<(String, AggFn, FieldPath)>, pub filter: Option<Expr>, pub having: Option<Expr> }
pub struct Join      { pub left: Scan, pub right: Scan, pub on: Vec<(FieldPath, FieldPath)>, pub kind: JoinKind /* Inner|Left */ , pub projection: Option<Vec<FieldPath>> }
pub enum Mutate {
    Insert      { c: ContainerRef, rows: Vec<Row>, on_conflict: OnConflict /* Error|Replace|Ignore */ },
    Update      { c: ContainerRef, set: Vec<(FieldPath, Expr)>, filter: Option<Expr> },
    DeleteWhere { c: ContainerRef, filter: Option<Expr> },
}
pub struct TypeOp    { pub c: ContainerRef, pub op: String, pub args: CanonicalValue }     // op ∈ Datatype::operations()
pub enum ObjectReq   { Put{..}, GetRange{..}, Head{..}, Copy{..}, Move{..}, ListPrefix{..}, MultipartBegin{..}, MultipartPart{..}, MultipartComplete{..}, MultipartAbort{..} }  // mirrors ObjectOps
```

### `Expr`

```rust
pub enum Expr {
    Field(FieldPath), Lit(CanonicalValue),
    Cmp(CmpOp /* Eq Ne Lt Le Gt Ge */, Box<Expr>, Box<Expr>),
    In(Box<Expr>, Vec<CanonicalValue>), Like { e: Box<Expr>, pattern: String, case_insensitive: bool }, Prefix(Box<Expr>, String),
    Range { e: Box<Expr>, from: Option<(CanonicalValue, bool /*inclusive*/)>, to: Option<(CanonicalValue, bool)> },
    And(Vec<Expr>), Or(Vec<Expr>), Not(Box<Expr>), Exists(FieldPath), IsNull(Box<Expr>),
    Func { name: String, args: Vec<Expr> },   // limited scalar functions (lower, upper, length, coalesce, now); NotSupported otherwise
}
```

## Mapping table: protocol construct → `LogicalRequest`

| Protocol construct | Request |
|---|---|
| SQL/CQL `CREATE TABLE … WITH/ENGINE type` | `Ddl::CreateContainer` |
| SQL/CQL `SELECT` single table | `Scan` or `Aggregate` (when `GROUP BY`/aggregate functions) |
| SQL `SELECT … JOIN` (2 tables) | `Join` |
| SQL/CQL `INSERT`, `UPDATE`, `DELETE` | `Mutate::*` (`INSERT … ON CONFLICT DO UPDATE` → `Replace`) |
| SQL `SELECT * FROM spacestorage.knn(...)` / CQL `spacestorage_knn` / CH `spacestorageKnn()` | `TypeOp{op:"knn"}` |
| Redis `GET/SET/DEL/EXISTS/MGET/MSET` | `Point::*` on session `target_container` |
| Redis `INCR/EXPIRE/TTL/HSET…` | `TypeOp` on `kv_collection` (`incr`, `expire`, `hset`, …) |
| Redis `KEYS/SCAN` | `Scan{projection:[key], filter: Like}` |
| Redis `SS.CONTAINERS` / `SS.CREATE` / `SS.DROP` | `Ddl::*` |
| ES `PUT /{index}` | `Ddl::CreateContainer` |
| ES `_doc` CRUD, `_bulk` | `Point::*` / `Batch` |
| ES `_search` | `Scan` or `Aggregate` |
| ES `_delete_by_query` | `Mutate::DeleteWhere` |
| ES `/{index}/_spacestorage/{op}` | `TypeOp` |
| ClickHouse `INSERT … FORMAT` | `Mutate::Insert` (blocks batched) |
| S3 bucket ops | `Ddl::*` (`ListBuckets` → `ListContainers`) |
| S3 object ops | `ObjectReq::*` |
| S3 `POST /{bucket}?spacestorage-op=` | `TypeOp` |
| WebDAV `MKCOL` (top-level) / `DELETE` collection | `Ddl::*` |
| WebDAV `PUT/GET/HEAD/DELETE/COPY/MOVE/PROPFIND` | `ObjectReq::*`; `PROPFIND /` → `ListContainers` |
| WebDAV `LOCK/UNLOCK` | `Point::*` on system `ordered_map` `.webdav_locks` |
| WebDAV `REPORT spacestorage:op` | `TypeOp` |

## `PlacementInfo`

```rust
pub trait PlacementInfo: Send + Sync {
    fn replicas(&self, c: &ContainerRef) -> u16;                       // LocalEngine: 1
    fn satisfiable(&self, q: QuorumLevel, c: &ContainerRef) -> bool;   // LocalEngine: q ∈ {ONE, LOCAL_ONE, Acks(1), QUORUM, LOCAL_QUORUM, EACH_QUORUM, ALL} → true; TWO/THREE/Acks(n>1) → false
    fn clamp(&self, q: QuorumLevel, c: &ContainerRef) -> QuorumLevel;  // highest satisfiable ≤ q; LocalEngine: Acks(1)
}
```

`QUORUM`-family levels are computed against `replicas()` (`floor(n/2)+1`), so on a single replica they are satisfiable.

## Interim `LocalEngine` (this feature)

- Single node, executes against `TypeRegistry` directly; `placement().replicas() == 1`.
- `Join`: nested-loop over two scans, `Inner`/`Left` only; larger plans → `NotSupported{what:"multi-way join"}`.
- `Batch`: sequential, stops at first error, no rollback (documented; `05` adds transactions).
- Timeout: `tokio::time::timeout` around each stage; cancellation drops the stream.
- Records: ring buffer `exec.records` (10 000 entries), admin op `executions`.

## Admin extension (this feature)

`admin`/`admin-http` gain read-only op `executions` → `GET /v1/executions?protocol=&namespace=&limit=100` returning `[ExecutionRecord]` (see [data-model.md §6](../data-model.md#6-execution-boundary-cratesexec)). Token-protected like all admin ops.
