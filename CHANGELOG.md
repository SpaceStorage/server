## 2026-10-28 — specs/015-compatibility-and-limits

- Stock-client wire conformance for Cassandra native, Elasticsearch HTTP, ClickHouse native and HTTP, S3 SigV4, and WebDAV, beside the existing dispatch checks.
- MUST NOT paths no longer succeed: Elasticsearch search and aggregations, ClickHouse materialized views and replicated engines, S3 bucket subresources and key-less object calls, WebDAV HEAD/OPTIONS, Cassandra LWT deletes and CDC, PostgreSQL FDW and forbidden COPY/cursors.
- Isolation `SET` applies Snapshot or refuses serializable; `ALTER TABLE` maps onto a container; `TypeDescriptor.introduced_in` refuses a type newer than the local product version.

## 2026-10-28 — specs/007-tenancy-security

- Namespace, quota, and role changes commit on the cluster majority, and the CLI calls admin instead of printing a request body.
- Quotas admit per datatype, with a one-second ops bucket, internodes usage deltas, rollback, and a repeating catalog reconcile. Writes and sessions on the tenant protocols run the same role, policy, and quota checks.
- Cassandra and ClickHouse namespace DDL use that commit path. A first-binary build does not turn on quota enforcement. Private metrics and logs export stays with feature 08.
