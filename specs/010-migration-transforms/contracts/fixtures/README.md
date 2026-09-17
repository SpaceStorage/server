# Fixtures

Contract tests parse these as `job.create` bodies (and the invalid/ cases as refused).

| File | Expect |
|------|--------|
| `evacuate-live.json` | `data_migration` live node-to-node |
| `namespace-copy.json` | copy `acme.t` → `beta.t` |
| `transform-catalog.json` | document_store → relational_table, no mapping query |
| `transform-mapping-query.json` | complex source with query |
| `backup-namespace.json` | `data_backup` namespace scope |
| `invalid/name-exists.json` | dest name taken → `NameExists` |
| `invalid/mapping-missing.json` | complex, no query → `MappingQueryRequired` |
| `invalid/quota.json` | dest quota too small → `QuotaExceeded` |
