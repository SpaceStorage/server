# Feature Specification: SpaceStorage Runtime, CLI, and Node Interfaces

**Feature Branch**: `001-runtime-cli-api`

**Created**: 2026-09-13

**Status**: Draft

**Input**: User description: "Read .specify/intent/01-runtime-cli-api.md and specify this feature." — the intent file describes the runtime and operator-facing control surfaces of the SpaceStorage database server: one async process per node with a configurable number of worker threads sized from the machine's core count, a bundled CLI shipped with the server, per-node TCP and HTTP interfaces that an administrator must explicitly enable, and per-node configurable buffer sizes whose usage is monitored.

## Clarifications

### Session 2026-09-13

- Q: What should a node do at startup when its configuration says nothing at all about the TCP interface or the HTTP interface? → A: Startup fails with an error naming the missing declaration; the administrator must explicitly enable or explicitly disable each interface. There is no implicit default for either interface.
- Q: Should the node's TCP interface carry only administrative operations, or also be a native data path for clients? → A: Neither as a monolithic "interface". Every listening port is declared as an **entrypoint** with exactly one **handler**, e.g. `entrypoint { port 9042; handler cassandra; }`. Handlers include `admin` (TCP administrative API used by the CLI), `admin-http` (HTTP administrative API), `postgresql`, `cassandra`, `clickhouse`, `elasticsearch`, `replication`, `syslog`, and so on. The "TCP interface" of the intent is the `admin` handler; the "HTTP interface" is the `admin-http` handler. Client data travels only through protocol handlers; the admin handlers are administrative-only. This feature owns the entrypoint model and the two admin handlers; other features register their own handlers.
- Q: Should connections to the `admin` and `admin-http` entrypoints be able to use transport encryption (TLS), and is it optional or required? → A: Optional per entrypoint. An entrypoint may declare TLS with certificate material referenced (never inlined); plaintext is allowed. The effective configuration flags each admin entrypoint as encrypted or plaintext.
- Q: When an administrator changes thread count, buffer sizes, or entrypoints, should the change apply only after a restart or live? → A: Buffers live, structure on restart. Buffer capacities and the drain timeout can be reloaded on a running node via the admin API and CLI; thread count and entrypoints require a restart.
- Q: How should the node pick its default number of worker threads from the machine's core count when the administrator does not set one? → A: One worker thread per available core (threads = cores), minimum 1.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Start a node sized to the machine (Priority: P1)

A cluster administrator installs SpaceStorage on a server, writes a node configuration, and starts the node. The node comes up as a single process, chooses its worker thread count from the number of cores on the machine (unless the administrator overrides it), reports that it is ready, and keeps serving concurrent requests without any one slow operation stalling the others. When the administrator stops the node, it finishes in-flight work within a drain window and exits cleanly.

**Why this priority**: Nothing else in the product exists until a node process can start, size itself, run, and stop predictably. This is the minimal viable runtime.

**Independent Test**: Can be fully tested by starting one node from a configuration file on machines with different core counts, observing the reported worker thread count and readiness state, issuing concurrent requests, and stopping the node. Delivers a runnable, sizable server process with no other features present.

**Acceptance Scenarios**:

1. **Given** a machine with N available cores and a configuration that does not set a thread count, **When** the node starts, **Then** the node reports exactly N worker threads (minimum 1) and reaches the `ready` state.
2. **Given** a configuration that sets the thread count explicitly to K, **When** the node starts, **Then** the node reports exactly K worker threads regardless of the machine's core count.
3. **Given** a configuration that sets an invalid thread count (zero, negative, or non-numeric), **When** the node starts, **Then** startup fails before any entrypoint is opened and the error names the offending setting and the accepted range.
4. **Given** a running node, **When** one request performs a long-running operation while many short requests arrive, **Then** the short requests complete without waiting for the long-running one to finish.
5. **Given** a running node with in-flight requests, **When** a stop signal is received, **Then** the node stops accepting new connections, enters the `draining` state, completes in-flight requests up to the configured drain timeout, and exits with a success status.
6. **Given** a node in `draining` state whose in-flight requests exceed the drain timeout, **When** the timeout elapses, **Then** the node terminates the remaining requests, records that the drain timed out, and exits.

---

### User Story 2 - Declare the node's entrypoints and enable the admin handlers (Priority: P1)

A cluster administrator decides which ports a node listens on and what each port speaks. Every listening port is declared as an entrypoint with exactly one handler, for example `entrypoint { port 9042; handler cassandra; }`. Two handlers belong to this feature: `admin` (the TCP administrative API used by the bundled CLI) and `admin-http` (the HTTP administrative API). For every node the administrator explicitly declares each of the two admin handlers as either enabled on a port or disabled. The node opens exactly the declared entrypoints and refuses to start when an admin handler has not been declared at all, so no node can accidentally run with an undeclared administrative surface. Once an admin entrypoint is up, the administrator can reach the node's built-in API through it: node identity, current state, effective configuration, thread and buffer usage.

**Why this priority**: The intent mandates that every administrator enables TCP and HTTP interfaces for each node. Without the admin entrypoints the node cannot be administered remotely and the CLI has nothing to connect to. Ties with Story 1 for MVP.

**Independent Test**: Can be fully tested by starting nodes with the four combinations of `admin`/`admin-http` enabled/disabled, a configuration that omits an admin handler, a configuration with an unknown handler, and admin entrypoints with and without TLS, then attempting to connect to each declared entrypoint. Delivers a remotely administrable node.

**Acceptance Scenarios**:

1. **Given** a configuration with an entrypoint on address A:port P with handler `admin` and an entrypoint on address B:port Q with handler `admin-http`, **When** the node starts, **Then** both listeners accept connections on exactly those addresses and the node's effective configuration lists both entrypoints with their handlers.
2. **Given** a configuration that declares `admin-http` as disabled and declares an `admin` entrypoint on a port, **When** the node starts, **Then** the `admin` entrypoint accepts connections, no `admin-http` listener is opened, and the node logs a startup notice that `admin-http` is disabled by administrator choice.
3. **Given** a configuration that does not mention the `admin` handler or the `admin-http` handler at all, **When** the node starts, **Then** startup fails and the error states that each admin handler must be explicitly enabled on an entrypoint or explicitly disabled by the administrator.
4. **Given** a configuration whose entrypoint port is already in use on the host, **When** the node starts, **Then** startup fails, the error names the entrypoint (address, port, handler) and the conflicting address, and no partially started listener remains open.
5. **Given** two entrypoints configured on the same address and port, **When** the node starts, **Then** startup fails with an error naming both conflicting entrypoints and their handlers.
6. **Given** a configuration with an entrypoint whose handler name is not in the node's handler inventory, or an entrypoint with no handler or more than one handler, **When** the node starts, **Then** startup fails and the error names the entrypoint and lists the known handlers.
7. **Given** a running node with an `admin-http` entrypoint, **When** an administrator requests node status over HTTP, **Then** the response includes node name, node state, uptime, worker thread count, per-buffer configured size and current usage, and the list of active entrypoints with their handlers.
8. **Given** a running node with an `admin` entrypoint, **When** the bundled CLI connects to it and requests node status, **Then** the CLI receives the same information as in scenario 7.
9. **Given** a running node with a protocol entrypoint (for example handler `cassandra`) and an `admin` entrypoint, **When** a client sends an administrative request to the protocol entrypoint or a data request to the `admin` entrypoint, **Then** the request is rejected by that entrypoint's handler; each port speaks only its declared handler.
10. **Given** an `admin-http` entrypoint declared with TLS and a reference to valid certificate material, **When** the node starts and an administrator connects, **Then** the connection is encrypted, a plaintext connection attempt to that entrypoint is refused, and the effective configuration flags the entrypoint as encrypted.
11. **Given** an admin entrypoint declared with TLS whose referenced certificate material is missing, unreadable, or expired, **When** the node starts, **Then** startup fails and the error names the entrypoint and the certificate reference; no plaintext fallback listener is opened.
12. **Given** an admin entrypoint declared without TLS, **When** the node starts, **Then** the entrypoint accepts plaintext connections and the effective configuration flags it as plaintext.

---

### User Story 3 - Operate a node locally with the bundled CLI (Priority: P2)

A local operator on a node uses the CLI that ships with the server. Before starting the node, the operator validates a configuration file and gets a clear pass/fail with the list of problems. After the node is up, the operator connects to it, inspects its state, effective configuration, worker thread utilization, and buffer usage, and initiates a graceful drain and stop. The CLI needs no separate installation and works against any live node that exposes an `admin` or `admin-http` entrypoint.

**Why this priority**: The CLI is the primary hands-on control surface for operators. It is essential for day-two operation but a node can be started and administered over HTTP without it, so it follows the two runtime stories.

**Independent Test**: Can be fully tested by running the CLI against a valid and an invalid configuration file (no node needed), then against a running node for status, configuration, thread, buffer, and stop commands. Delivers a complete local operator workflow.

**Acceptance Scenarios**:

1. **Given** a server installation, **When** the operator lists the installed files, **Then** the CLI is present as part of the same installation with no additional download or install step.
2. **Given** a configuration file with an unknown setting, a thread count of zero, and a missing `admin` handler declaration, **When** the operator runs the CLI validation command, **Then** the CLI exits with a non-zero status and lists all three problems with the setting names in one run.
3. **Given** a valid configuration file, **When** the operator runs the CLI validation command, **Then** the CLI exits with a zero status and prints the effective configuration that the node would apply, including the derived thread count for the current machine.
4. **Given** a running node, **When** the operator runs the CLI status command, **Then** the CLI prints node name, node state, uptime, worker thread count, busy thread count, and per-buffer configured size and current usage.
5. **Given** a running node, **When** the operator runs the CLI stop command, **Then** the node begins draining and the CLI reports completion once the node has exited or the drain timeout has elapsed.
6. **Given** a node whose admin entrypoints are unreachable, **When** the operator runs any CLI command that targets that node, **Then** the CLI fails within its connection timeout and the message names the address it tried and the handler (`admin` or `admin-http`) it used.
7. **Given** the CLI is asked to output machine-readable results, **When** any inspection command runs, **Then** the output is structured (fields with stable names) so scripts can consume it without parsing human-oriented text.
8. **Given** a running node and an updated configuration file that changes only buffer capacities and the drain timeout, **When** the operator runs the CLI reload command, **Then** the node applies the new values without restarting, existing connections stay open, and the CLI prints which settings changed.
9. **Given** a running node and an updated configuration file that also changes the thread count or an entrypoint, **When** the operator runs the CLI reload command, **Then** the node applies the live-reloadable settings, reports the structural settings as pending restart, and the CLI exits with a status that distinguishes "applied with restart pending" from "fully applied".

---

### User Story 4 - Size node buffers and see how full they are (Priority: P2)

A cluster administrator tunes memory use per node by setting the size of each named buffer the node owns. The node validates the sizes on startup or on reload, applies them without a restart, and continuously tracks how much of each buffer is in use and how often a buffer hit its limit. The administrator reads these figures through the `admin-http` entrypoint, the `admin` entrypoint, or the CLI to decide whether a buffer needs to be resized. When a buffer reaches its limit, the node applies a defined overflow behavior instead of growing without bound.

**Why this priority**: Buffer sizing is how operators fit a node to its machine and keep it stable under load, but it is only meaningful once the runtime and admin entrypoints exist.

**Independent Test**: Can be fully tested by starting a node with custom buffer sizes, generating load that fills a buffer, and reading usage figures through each admin entrypoint and the CLI. Delivers observable, bounded memory use per node.

**Acceptance Scenarios**:

1. **Given** a configuration that sets sizes for several named buffers, **When** the node starts, **Then** the effective configuration reports each buffer with the configured size, and buffers not mentioned report their default size.
2. **Given** a configuration that names a buffer the node does not have, or sets a size outside the accepted range, **When** the node starts, **Then** startup fails and the error names the buffer and the accepted range or the list of known buffers.
3. **Given** a running node under load, **When** an administrator reads buffer usage, **Then** each buffer reports current usage in bytes and as a percentage of its configured size, and the figures change as load changes.
4. **Given** a buffer that has reached its configured size, **When** more data arrives for it, **Then** the node applies the buffer's declared overflow behavior (reject with a clear error, or wait until space frees up, as declared for that buffer), and the buffer's limit-hit count increases by one.
5. **Given** two nodes in the same cluster, **When** the administrator sets different buffer sizes on each, **Then** each node applies only its own sizes and reports only its own usage.
6. **Given** a running node under load, **When** the administrator raises a buffer's capacity via reload, **Then** the new capacity is reported within one reporting interval, in-flight requests are unaffected, and usage percentage is recomputed against the new capacity.
7. **Given** a running node whose buffer currently holds more data than a newly requested smaller capacity, **When** the administrator lowers that buffer's capacity via reload, **Then** the node accepts the new capacity, stops admitting new data into the buffer until usage falls below the new capacity, reports usage above 100% in the meantime, and does not discard data already in the buffer.
8. **Given** a reload request containing an out-of-range buffer capacity or an unknown buffer name, **When** the node validates it, **Then** the entire reload is rejected, no setting changes, and the error names the offending buffer.

---

### Edge Cases

- Thread count set higher than the core count: accepted, applied as configured, and reported so the administrator can see the oversubscription.
- Machine core count cannot be determined: the node falls back to a documented minimum thread count and logs the fallback.
- Entrypoint bound to an address the host does not own: startup fails with the entrypoint (address, port, handler) in the error; no listener is left open.
- Entrypoint declared without a port, without a handler, or with more than one handler: startup fails naming the entrypoint; exactly one handler per entrypoint.
- The same handler declared on several entrypoints (for example `admin` on a loopback port and on a management-network port): accepted; the handler serves all of its entrypoints and each is listed separately in the effective configuration.
- Entrypoint declared with a handler that belongs to a feature not yet available on this build: startup fails and the error lists the handlers this build knows.
- Certificate material for a TLS entrypoint is inlined in the configuration instead of referenced: startup fails; the error states that certificate material must be referenced, not inlined.
- CLI connects to a TLS admin entrypoint whose certificate is not trusted by the operator's machine: the CLI fails with a message naming the entrypoint and the trust problem; it does not silently downgrade to plaintext.
- Certificate material of a running TLS entrypoint expires while the node is up: existing connections continue; new connections fail with a certificate error; the node logs the expiry and the effective configuration flags the entrypoint's certificate as expired.
- Stop signal received during startup, before `ready`: the node aborts startup, closes any listener it had opened, and exits without entering `ready`.
- Second stop signal during draining: the node terminates immediately without waiting for the drain timeout.
- Configuration file unreadable, malformed, or absent: startup fails with the file path and the parse problem; the CLI validation command reports the same problem.
- Buffer size set to zero: rejected as out of range unless the buffer's documented range allows a zero-size (disabled) buffer.
- Buffer sizes whose sum exceeds available machine memory: the node starts (or applies the reload) but records and logs a warning naming the total configured buffer memory and the available memory.
- Two reload requests arrive concurrently: the node applies them one at a time in arrival order; each response reports the effective configuration after that request.
- Reload requested while the node is `draining`: rejected with the node state in the error; no setting changes.
- Reload changes the drain timeout while a drain is not in progress: the new timeout applies to the next stop request.
- CLI targets a node whose admin entrypoint is up but whose node state is `starting` or `draining`: the CLI receives the state and does not treat it as a connection failure.
- Identical requests arriving at an `admin` entrypoint and an `admin-http` entrypoint: both return the same information; neither handler exposes information the other cannot.

## Requirements *(mandatory)*

### Functional Requirements

**Process and runtime**

- **FR-001**: Each SpaceStorage node MUST run as exactly one operating-system process that serves all of the node's entrypoints, handlers, and data work.
- **FR-002**: The node MUST run a pool of worker threads and, when no thread count is configured, MUST size it to one worker thread per core available to the process (threads = available cores, minimum 1).
- **FR-003**: The administrator MUST be able to override the worker thread count with an explicit value; the node MUST reject values below one and MUST report the effective count.
- **FR-004**: Work performed for one request MUST NOT block progress of unrelated requests on the same node; waiting on network, disk, timers, or other requests MUST yield capacity to other work.
- **FR-005**: The node MUST expose its lifecycle state as one of at least `starting`, `ready`, `draining`, `failed`; state names MUST match the node-state vocabulary of the observability intent.
- **FR-006**: On a stop request the node MUST stop accepting new connections, complete in-flight requests within a configurable drain timeout, then exit; requests still running after the timeout MUST be terminated and the timeout MUST be recorded.
- **FR-007**: A stop request received before the node is `ready` MUST abort startup, release any opened listener, and exit without entering `ready`.

**Configuration**

- **FR-008**: The node MUST read its configuration from a declarative configuration source supplied by the administrator, and MUST allow individual settings to be overridden at launch.
- **FR-009**: The node MUST validate the entire configuration before opening any listener and MUST fail startup on the first validation pass with all detected problems listed, each naming the setting involved.
- **FR-010**: The node MUST report its effective configuration (configured values with defaults and derivations resolved) through every admin entrypoint and the CLI.
- **FR-011**: Thread count, drain timeout, entrypoint declarations, and buffer sizes MUST be settable independently per node; no setting in this feature is cluster-wide.
- **FR-012**: Every setting in this feature MUST be classified as either live-reloadable or restart-required, and the classification MUST appear in the documentation and in the effective configuration. Buffer capacities and the drain timeout are live-reloadable; worker thread count and entrypoint declarations (address, port, handler, TLS) are restart-required.
- **FR-013**: The node MUST accept a reload request through the admin handlers that re-reads the configuration source, validates it as a whole (FR-009), and applies all live-reloadable changes atomically without dropping connections or in-flight requests; if validation fails, no setting changes.
- **FR-014**: When a reload contains restart-required changes, the node MUST apply the live-reloadable part, MUST report each restart-required setting as pending restart in the effective configuration, and MUST keep running with its current structural settings.
- **FR-015**: The node MUST reject a reload while it is `draining` or before it is `ready`, naming the node state in the error.

**Entrypoints and handlers**

- **FR-016**: Every listening port of a node MUST be declared as an entrypoint consisting of a port, an optional listen address, and exactly one handler (for example `entrypoint { port 9042; handler cassandra; }`). The node MUST NOT open any listener that is not declared as an entrypoint.
- **FR-017**: The node MUST keep an inventory of known handler names. This feature defines the handlers `admin` (TCP administrative API) and `admin-http` (HTTP administrative API). Other features register additional handlers (for example `postgresql`, `cassandra`, `clickhouse`, `elasticsearch`, `redis`, `s3`, `webdav`, `replication`, `syslog`); the inventory is expandable and MUST be listed in the node's documentation and effective configuration.
- **FR-018**: An entrypoint whose handler is not in the inventory, has no handler, or has more than one handler MUST fail startup with an error naming the entrypoint and listing the known handlers.
- **FR-019**: Each entrypoint MUST speak only its declared handler; requests for another handler arriving at that port MUST be rejected by the declared handler. The same handler MAY be declared on several entrypoints.
- **FR-020**: Every node MUST offer the `admin` and `admin-http` handlers as part of the server itself, exposing the same administrative capabilities (status, effective configuration, thread usage, buffer usage, stop) over both.
- **FR-021**: For each of the two admin handlers the administrator MUST explicitly declare either an entrypoint (enabled) or a disabled state; a configuration that omits either declaration MUST fail startup with an error naming the missing handler declaration.
- **FR-022**: For each entrypoint the node MUST open a listener on exactly the declared address and port and on no other; when no address is declared, the node MUST apply a documented default address and report it in the effective configuration.
- **FR-023**: The node MUST refuse to start when two entrypoints are configured on the same address and port, or when a configured port is already in use, and MUST name the conflicting entrypoints (address, port, handler) in the error.
- **FR-024**: A disabled admin handler MUST NOT open a listener, and the node MUST log at startup that the handler is disabled by administrator choice.
- **FR-025**: The admin handlers MUST report node name, node state, uptime, worker thread count, busy worker thread count, the list of active entrypoints with their handlers, and per-buffer configured size, current usage in bytes and percent, and limit-hit count.
- **FR-026**: Access to the admin handlers MUST be subject to the cluster role system; unauthenticated callers MUST NOT be able to change node state or read effective configuration. Authentication mechanics are specified in the tenancy and security feature.
- **FR-027**: An entrypoint MAY declare transport encryption (TLS). When declared, the certificate material MUST be referenced (for example by path or secret-store location) and MUST NOT be inlined in the configuration; the entrypoint MUST refuse plaintext connections. When not declared, the entrypoint accepts plaintext connections.
- **FR-028**: A TLS entrypoint whose referenced certificate material is missing, unreadable, or expired at startup MUST fail startup with an error naming the entrypoint and the reference; the node MUST NOT fall back to plaintext.
- **FR-029**: The effective configuration MUST flag every entrypoint as `encrypted` or `plaintext`, and the admin handlers and CLI MUST show this flag alongside each active entrypoint.
- **FR-030**: The CLI MUST support connecting to TLS admin entrypoints, MUST verify the server certificate against the operator's trust configuration, and MUST NOT downgrade to plaintext when verification fails.

**Bundled CLI**

- **FR-031**: The server distribution MUST include the CLI; installing the server MUST make the CLI available without a separate install.
- **FR-032**: The CLI MUST validate a configuration file without a running node and MUST list every problem found in one run, exiting with a non-zero status when any problem exists.
- **FR-033**: The CLI MUST connect to a node through an `admin` entrypoint or an `admin-http` entrypoint, selectable by the operator, and MUST use a connection timeout after which it fails with the target address and handler named.
- **FR-034**: The CLI MUST provide, at minimum, commands to show node status, show effective configuration (including entrypoints and handlers), show worker thread usage, show buffer usage, request a graceful stop, and request a configuration reload.
- **FR-035**: Every CLI inspection command MUST offer a structured, machine-readable output mode with stable field names in addition to human-readable output.
- **FR-036**: CLI exit status MUST distinguish success, validation failure, connection failure, node-reported error, and "applied with restart pending" after a reload that contained restart-required changes.
- **FR-037**: After a reload the CLI MUST print which settings changed, which were applied live, and which are pending restart.

**Buffers**

- **FR-038**: The node MUST maintain a registry of named buffers; each buffer MUST have a name, a configured capacity, a documented accepted range, a default capacity, and a declared overflow behavior (reject or wait).
- **FR-039**: The administrator MUST be able to set the capacity of each named buffer per node at startup and via reload; unknown buffer names and out-of-range capacities MUST fail startup or reject the reload with the buffer name and accepted range or known-buffer list in the error.
- **FR-040**: Raising a buffer's capacity via reload MUST take effect without affecting in-flight requests. Lowering a buffer's capacity below its current usage MUST be accepted; the node MUST stop admitting new data into that buffer until usage falls below the new capacity, MUST report usage above 100% meanwhile, and MUST NOT discard data already held.
- **FR-041**: The node MUST track, for every buffer, current usage in bytes, usage as a percentage of capacity, and a count of limit hits, and MUST make these figures available through the admin handlers and CLI and to the metric catalog defined by the observability feature.
- **FR-042**: When a buffer is at capacity the node MUST apply that buffer's declared overflow behavior and MUST NOT grow the buffer beyond its configured capacity.
- **FR-043**: Other features that introduce buffers MUST register them in the same registry so they are configured, tracked, and reported the same way.
- **FR-044**: The node MUST warn at startup and on reload when the sum of configured buffer capacities exceeds the memory available on the machine, naming both figures.

### Key Entities

- **Node**: One SpaceStorage server process on one machine. Attributes: node name, lifecycle state, uptime, worker thread pool, set of entrypoints, handler inventory, buffer registry, effective configuration.
- **Node Configuration**: The administrator's declared settings for one node: thread count (optional override), drain timeout, list of entrypoints, explicit enabled/disabled declaration for each admin handler, per-buffer capacities, launch-time overrides. Validated as a whole before the node opens any listener.
- **Entrypoint**: One declared listening port of a node. Attributes: port, optional listen address (documented default when omitted), exactly one handler, optional TLS declaration (reference to certificate material, never inlined), resolved transport flag (`encrypted` or `plaintext`). Uniqueness: address plus port. Several entrypoints may share a handler.
- **Handler**: A named behavior that an entrypoint speaks. This feature defines `admin` (TCP administrative API) and `admin-http` (HTTP administrative API); other features add protocol handlers (`postgresql`, `cassandra`, `clickhouse`, `elasticsearch`, `redis`, `s3`, `webdav`), `replication`, `syslog`, and more. Attributes: name, owning feature, whether it is administrative or client-facing. The inventory is expandable.
- **Admin Handler Declaration**: The administrator's explicit statement for `admin` and for `admin-http`: an entrypoint (enabled) or disabled. Both declarations are mandatory.
- **Worker Thread Pool**: The set of threads executing node work. Attributes: configured or derived size, count currently busy.
- **Buffer**: A named, bounded memory region owned by the node. Attributes: name, default capacity, accepted range, configured capacity, current usage (bytes and percent), limit-hit count, overflow behavior (reject or wait).
- **Effective Configuration**: The resolved view of a node's configuration after defaults, derivations (such as thread count from cores, default listen address), and overrides are applied, including the resolved entrypoint list; each setting carries its reload class (live-reloadable or restart-required) and, after a reload, a pending-restart marker where the on-disk value differs from the running value. Reported identically over `admin`, `admin-http`, and the CLI.
- **Reload Request**: One administrator-initiated re-read of the configuration source on a running node. Attributes: validation result, list of settings applied live, list of settings pending restart, node state at the time of the request.
- **CLI Session**: One invocation of the bundled CLI: target node and admin handler, command, output mode, connection timeout, exit status.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An administrator with a valid configuration can start a node and see it reach `ready` in under 10 seconds on a machine with no existing data.
- **SC-002**: On 100% of tested machines with differing core counts, a node started without an explicit thread count reports a worker thread count equal to the number of cores available to the process (minimum 1).
- **SC-003**: With one long-running request held open, at least 99% of concurrent short requests complete within twice their unloaded latency.
- **SC-004**: 100% of invalid configurations in the acceptance test set (bad thread count, missing admin handler declaration, unknown handler, entrypoint without exactly one handler, unknown buffer, out-of-range size, port conflict, malformed file) are rejected before any listener opens, and every rejection message names the offending setting.
- **SC-005**: Every declared entrypoint accepts a connection within 1 second of the node reporting `ready`; a declared-disabled admin handler never accepts a connection; no undeclared port is open on the node.
- **SC-006**: 100% of plaintext connection attempts to a TLS-declared admin entrypoint are refused, and 100% of TLS-declared entrypoints with invalid certificate references are rejected at startup with no listener opened.
- **SC-007**: An operator can validate a configuration, start a node, read its status, and stop it gracefully using only the bundled CLI in under 5 minutes on first attempt, following the starter documentation.
- **SC-008**: Buffer usage read through `admin`, `admin-http`, and CLI agrees to within one reporting interval, and a buffer driven to its limit shows a limit-hit count increase and holds its usage at or below 100% of configured capacity.
- **SC-009**: A reload that changes only buffer capacities and drain timeout is applied on a loaded node with 0 dropped connections and 0 failed in-flight requests in 100% of test runs, and the new values are visible through `admin`, `admin-http`, and CLI within one reporting interval.
- **SC-010**: A node with in-flight requests completes a graceful stop within the configured drain timeout plus 1 second in 100% of test runs.
- **SC-011**: Starter documentation includes at least one complete example node configuration covering thread count, an `admin` entrypoint, an `admin-http` entrypoint, at least one protocol entrypoint (for example `entrypoint { port 9042; handler cassandra; }`), drain timeout, and buffer sizes, and the example passes CLI validation unchanged.

## Assumptions

- The intent's "TCP interface" and "HTTP interface" are the `admin` and `admin-http` handlers: two transports for the same built-in administrative API of the node, each declared as an entrypoint; the bundled CLI uses either. They are administrative-only and distinct from the client-facing protocol handlers (`postgresql`, `cassandra`, `redis`, and others) specified in the protocols feature, which are declared as their own entrypoints on their own ports (confirmed in Clarifications, Session 2026-09-13).
- "Every administrator MUST enable TCP and HTTP interfaces for each node" is read as: both admin handlers exist on every node, neither is on implicitly, and the administrator must make an explicit enabled/disabled declaration for each. Omitting a declaration is a configuration error (confirmed in Clarifications, Session 2026-09-13); explicitly disabling is allowed and logged.
- Handler names used in this spec (`admin`, `admin-http`, `postgresql`, `cassandra`, `clickhouse`, `elasticsearch`, `redis`, `s3`, `webdav`, `replication`, `syslog`) are the canonical configuration vocabulary; the protocols, distribution, control-plane, and ingest features define the behavior behind their handlers and may add more names. The default listen address when an entrypoint omits one is a documented choice of the planning phase.
- Worker thread count defaults to one worker thread per core available to the process, minimum 1 (confirmed in Clarifications, Session 2026-09-13). "Available" means cores the process is allowed to use (respecting container or affinity limits), not the physical count of the host. The administrator may override it with any value of one or more, including values above the core count.
- Buffer capacities and the drain timeout are live-reloadable; worker thread count and entrypoint declarations take effect on node restart (confirmed in Clarifications, Session 2026-09-13). Reload re-reads the same configuration source the node started from; editing individual settings through the admin API without touching the configuration source is not part of this feature.
- The drain timeout has a documented default (on the order of tens of seconds) and is per node.
- Authentication and authorization for the admin handlers come from the cluster role system specified in the tenancy and security feature; this feature only requires that the admin handlers are governed by it. Until that feature exists, restricting the admin entrypoint listen address to a management network or loopback, and declaring TLS on the entrypoint, are the administrator's mitigations.
- Transport encryption is optional per entrypoint (confirmed in Clarifications, Session 2026-09-13). Certificate material is referenced from a path or secret store and never inlined in the configuration; which reference forms are supported is a planning decision. The TLS option applies to protocol entrypoints too, but each protocol feature decides whether its wire protocol can carry TLS.
- The buffer registry in this feature covers node-level buffers (at minimum: inbound request queue and per-entrypoint network send/receive buffers). Buffers introduced by storage, cache, query, or replication features register into the same registry; the exact buffer inventory is expandable.
- Metric series names, labels, and the scrape endpoint for buffer usage, worker thread usage, node state, and uptime are owned by the observability feature. This feature guarantees the underlying figures are tracked and exposed to it.
- Node state restoration on boot, cluster membership, and controller elections belong to the control plane feature; `ready` in this feature means the local runtime and declared entrypoints are up, and later features may add preconditions to `ready`.
- Starter documentation with example configurations is a deliverable of this feature, consistent with the constitution's documented-configuration principle.

## Out of Scope

The intent file assigns the following to sibling features. This specification does not define them; it only reserves the touchpoints listed.

- **Wire protocols and drivers** for PostgreSQL, Cassandra, Redis, Elasticsearch, ClickHouse, S3, and WebDAV (intent `02`). This feature reserves their handler names in the entrypoint inventory and requires that each protocol runs on its own declared entrypoint; the behavior behind each handler is specified there.
- **Type inventories** and the L0–L4 type stack (intent `03`).
- **Cluster topology, membership, controller elections, and state restore on boot** (intent `06`). This feature's `ready` state covers the local runtime and entrypoints only.
- **Metric series catalog**: metric names, labels, and the scrape endpoint for node state, uptime, thread usage, and buffer usage (intent `08`). This feature tracks and exposes the underlying figures; the catalog names them.
- **Graphical admin UIs and log ingest** (intent `09`). The `syslog` handler name is reserved only.
- **Authentication and authorization mechanics** for the admin handlers (intent `07`). This feature requires that the admin handlers are governed by the cluster role system and nothing more.
- **Editing individual settings through the admin API** without changing the configuration source. Reload re-reads the source; there is no per-setting write API in this feature.
