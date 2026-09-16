# Data Model: Query Execution

**Feature**: `005-query-execution` | **Date**: 2026-09-16 | **Source**: [spec.md](spec.md) Key Entities, [research.md](research.md)

Types in Rust-ish notation. Crate owner is `crates/exec` unless noted. `LogicalRequest`, `QueryOptions`, `QueryEngine`, `ExecutionRecord`, `PlacementInfo` from `002` are **extended**, not replaced.

## 1. Isolation, concurrency, partial results (`QueryOptions` additions)

```text
enum IsolationLevel { ReadCommitted, Snapshot }

enum Concurrency { Sequential, Parallel { degree: u16 } }   // default Sequential (degree 1)

struct QueryOptions {                    // 002 fields remain
    ...
    isolation: Sourced<IsolationLevel>,  // SQL default ReadCommitted; ignored on auto-commit Redis
    concurrency: Sourced<Concurrency>,
    partial_ok: Sourced<bool>,           // default false; Query|Session|Native|GlobalDefault
    async_job: Sourced<bool>,            // default false; start as background job
}
```

Validation: unknown isolation text → protocol error, session unchanged. `serializable` → `ExecError::NotSupported` (never stored). `Snapshot` + type without snapshot → `snapshot_unsupported`. Unknown `partial`/`async` token → protocol error, session unchanged. Omitted `partial_ok` is off.

## 2. IR extensions (`LogicalRequest`)

Existing variants (`Ddl`, `Point`, `Scan`, `Aggregate`, `Join`, `Mutate`, `TypeOp`, `Object`, `Batch`) stay. Additions:

```text
enum LogicalRequest {
    ...
    Explain { inner: Box<LogicalRequest> },
    CopyIn  { c: ContainerRef, format: CopyFormat, rows: StreamId },
    CopyOut { c: ContainerRef, format: CopyFormat, filter: Option<Expr> },
    TxnBegin { isolation: IsolationLevel },
    TxnCommit,
    TxnRollback,
    SubscribeWait { exec_id: ExecId },
    Cancel { exec_id: ExecId },
}

enum CopyFormat { Text, Csv, Binary }    // PostgreSQL COPY formats in 015 MUST
```

`Batch` remains sequential auto-commit until a session txn is open; inside a txn, batch elements share the txn.

MUST NOT verbs never become a `LogicalRequest` (handler returns not-supported). First-binary COPY/BEGIN never become IR either (dialect profile).

## 3. Logical and physical plan

```text
struct LogicalPlan {
    id: PlanId,
    root: LogicalRequest,                // after bind
    containers: Vec<ContainerRef>,
    ops: Vec<TypeOpName>,
    isolation: IsolationLevel,
    concurrency: Concurrency,
    timeout: Sourced<Duration>,
    quorum: Sourced<QuorumLevel>,
}

enum EngineKind { Scan, Point, Mutate, NestedLoopJoin, HashJoin, Aggregate, Map, Reduce, Shuffle, Copy, Ddl }

struct PhysicalPlan {
    logical: PlanId,
    tasks: Vec<Task>,
    edges: Vec<(TaskId, TaskId)>,        // shuffle / dependency
}

struct Task {
    id: TaskId,
    engine: EngineKind,
    placement: Vec<NodeName>,            // ranked
    shard: Option<ShardId>,
    estimated_memory: ByteCount,
}
```

EXPLAIN serializes `LogicalPlan` (minimum) and MAY include `PhysicalPlan` engines and placement. It MUST NOT run tasks.

## 4. Execution record (inspectable state)

```text
enum ExecutionStage {
    Received, Bound, Planned, Scheduled,
    Running { task: TaskId },
    Finalizing, Done, Failed, Cancelled,
}

struct ExecutionRecord {                 // 002 fields remain
    ...
    stage: ExecutionStage,
    plan_id: Option<PlanId>,
    isolation_applied: Option<IsolationLevel>,
    concurrency_applied: Concurrency,
    coordinator: NodeName,
    replicas_contacted: Vec<NodeName>,
    acks_durable: u16,
    acks_memory: u16,
    elapsed: Duration,
    spill_bytes: u64,
}
```

Admin `GET /v1/executions` already exists; this feature adds the new fields. Ring buffer `exec.records` unchanged in size default (10 000).

## 5. Transaction

```text
struct Transaction {
    id: TxnId,
    session: SessionId,
    protocol: ProtocolName,              // bound; switching protocol = new session
    isolation: IsolationLevel,
    snapshot: Option<SnapshotToken>,     // from type/storage when Snapshot
    participants: Vec<ContainerRef>,
    distributed: bool,                   // true → 004 2PC on commit
    state: TxnState,
}

enum TxnState { Open, Preparing, Committed, Aborted, InDoubt }
```

Transitions: Open → (Commit → Preparing → Committed | Aborted) | (Rollback → Aborted). Preparing → InDoubt if a participant is unreachable past `txn_timeout` (`004`); recovery is `004`'s. No partial commit visible (FR-020).

Deadlock: `ExecError::RetryableTxn { reason: Deadlock | Conflict }` ; no writes from the aborted txn remain.

## 6. Subscription / job

```text
struct Subscription {
    exec_id: ExecId,
    principal: Principal,
    namespace: NamespaceName,
    state: JobState,                     // Accepted | Running | Succeeded | Failed | Cancelled
    created: Timestamp,
}

enum JobState { Accepted, Running, Succeeded, Failed, Cancelled }
```

Cancel of any wait surface (SQL/CQL/Redis/HTTP/admin) cancels the underlying execute token (FR-014). All surfaces name the same `exec_id`.

## 7. Admission and spill

```text
struct AdmissionLimits {
    max_concurrent_per_node: u32,        // default 512
    max_concurrent_per_namespace: u32,   // default 128
    max_memory: ByteCount,               // default 256MiB
    spill: bool,                         // default false
}

struct AdmissionToken {                  // RAII; drop releases counts
    node_slot: (),
    ns_slot: (),
    memory_reservation: ByteCount,
}

struct SpillDir { path: PathBuf }        // {data_dir}/spill/{exec_id}/ ; drop removes
```

Errors: `admission_rejected{limit: "node"|"namespace"|"memory"|"spill_disk", current, max}`.

## 8. Rank key

```text
struct RankKey {
    ladder_distance: u8,                 // index of first differing ladder key; missing = MAX
    rtt: Option<Duration>,               // None sorts after Some
    skew_unhealthy: bool,                // true deprioritized
}
```

Compared in that field order. Input from `placement` topology + `internode` gauges. HLC values from different `quorum_domain`s MUST NOT enter `skew_unhealthy`.

## 9. Unavailability

```text
struct PartUnavailable {
    container: ContainerRef,
    shard: Option<ShardId>,
    partition: Option<PartitionId>,
    live_replicas: u16,
    required_level: QuorumLevel,
}
```

Distinct from empty `RowBatch`. With `partial_ok.value == true`, remaining shards MAY still produce rows plus this error as a warning event and `Done.partial = true`; without it the terminal event is `Error(Unavailable(_))`. Never a silent empty success and never `complete` on a partial.

## 10. State machines (summary)

**Query**: Received → Bound → Planned → (EXPLAIN: Done) → Scheduled → Running ↔ Running → Finalizing → Done | Failed | Cancelled.

**Txn**: see §5.

**Job**: Accepted → Running → Succeeded | Failed | Cancelled.

Invalid transitions are programming errors (panic in debug, `Failed` in release).
