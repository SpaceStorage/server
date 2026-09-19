# Contract: Planner and Executor Stages

**Feature**: `005-query-execution` | Crate: `crates/exec` | Spec: FR-005–FR-009, FR-025, FR-006

## Stages

Every `execute` call (except subscribe-accepted) walks:

1. **Received** — options snapshotted (reload does not change in-flight).
2. **Bound** — prepared param slots filled; ad-hoc is a no-op bind.
3. **Planned** — type catalog match (`003`); L4 walk; attach timeout/quorum/isolation/concurrency; build `LogicalPlan` + `PhysicalPlan`.
4. **Scheduled** — admission token acquired; tasks ranked (`coordination.md`).
5. **Running** — tasks executed via `placement::Coordinator` and local engines.
6. **Finalizing / Done | Failed | Cancelled**.

`ExecutionRecord.stage` is visible on `GET /v1/executions/{id}` and `spacestorage executions`.

### Parser-stage ownership (FR-005)

SQL/protocol parsers remain in handler crates (`plan.md` Complexity Tracking). **Handler parse → `LogicalRequest` is stage `Received`/`Bound` ownership in the handler crate** (wire decode, prepare/bind slots, dialect refuse before IR). From `Planned` onward, `ExecutionRecord` **always** names the shared planner stages above — never a protocol-private engine name. Acceptance for T019/T032 asserts those shared stage names (and `engine = planner`) on the record.

## Type match (FR-006)

Before touching data, every operation is checked against `Datatype::operations()`. Failure: `unsupported_by_type{container, type, op}` and no mutation.

L4: planner asks the composition for the member that owns the key (`003` write/read rules). Cycles cannot appear (catalog refuses them); if a stale cycle is seen, `Failed{cyclic_composition}` without execution.

## EXPLAIN (FR-025)

`LogicalRequest::Explain` completes at **Planned**. Response: containers, ops, engines, attached options, shard fan-out. JSON via admin `POST /v1/explain`. SQL: `EXPLAIN` text. MUST NOT verbs: not-supported, same as execute. First binary MAY; complete product MUST.

## Default concurrency (FR-008)

`Concurrency::Sequential`. `Parallel { degree }` is honoured only when the physical plan has independent tasks (multi-shard scan, map tasks). Degree is capped by remaining admission slots.

## Engine identity (SC-001)

`ExecutionRecord` MUST name `engine = planner` (not `local` / not a protocol name). `LocalEngine` tests set `engine = local` and are not first-binary conformance.
