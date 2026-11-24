## 2026-11-23 — specs/019-s3-protocol

- Complete-product S3 dialect on a dedicated entrypoint: SigV4 (header, presigned, streaming payload modes), namespace-bound credentials, path-style and virtual-hosted addressing, bucket/object CRUD, listing, CopyObject, DeleteObjects, and multipart with a durable upload registry.
- Streaming puts/gets and UploadPart in chunk windows without requiring a full-object buffer; fail-closed quorum/timeout options and durable `exec` acknowledgements on supported writes and deletes.
- Suite A `dialect_s3_*` conformance against a live server; Suite B Docker harness for rclone, AWS CLI, boto3, s3fs, clickhouse-backup, Elasticsearch snapshots, pgBackRest, Restic, plus aws-sdk-go-v2 smoke, with SHA256 verify on writes.
- Three-status compatibility matrix (`docs/s3-compatibility-matrix.{md,json}`) regenerated from the capability table and harness results. FirstBinary still forbids S3. Converge clean through Phase 19.

## 2026-11-22 — specs/018-admin-ui-modernization

- Modern admin shell on `/ui` (Overview, Namespaces, Nodes, Monitoring, Jobs, Audit) with permanent redirects from `/ui/cluster` and `/ui/console` into hosted `009` map/browse views.
- Product goal fixed as Cerebro/Kibana-class operator capability over SpaceStorage entities (namespace, container, shard, replica, job, principal) — not an Elasticsearch UI clone.
- Session moves to an `HttpOnly; Secure; SameSite=Strict` cookie with Origin/CSRF on cookie-authenticated mutations; `audit_list` and drain/undrain admin ops; server-side tenant isolation for namespace-scoped principals.
- Converge clean after Phases 11–14; UI conformance suites under `crates/conformance/tests/ui_*.rs`.

## 2026-10-28 — specs/015-compatibility-and-limits

- Stock-client wire conformance for Cassandra native, Elasticsearch HTTP, ClickHouse native and HTTP, S3 SigV4, and WebDAV, beside the existing dispatch checks.
- MUST NOT paths no longer succeed: Elasticsearch search and aggregations, ClickHouse materialized views and replicated engines, S3 bucket subresources and key-less object calls, WebDAV HEAD/OPTIONS, Cassandra LWT deletes and CDC, PostgreSQL FDW and forbidden COPY/cursors.
- Isolation `SET` applies Snapshot or refuses serializable; `ALTER TABLE` maps onto a container; `TypeDescriptor.introduced_in` refuses a type newer than the local product version.

## 2026-10-28 — specs/007-tenancy-security

- Namespace, quota, and role changes commit on the cluster majority, and the CLI calls admin instead of printing a request body.
- Quotas admit per datatype, with a one-second ops bucket, internodes usage deltas, rollback, and a repeating catalog reconcile. Writes and sessions on the tenant protocols run the same role, policy, and quota checks.
- Cassandra and ClickHouse namespace DDL use that commit path. A first-binary build does not turn on quota enforcement. Private metrics and logs export stays with feature 08.
