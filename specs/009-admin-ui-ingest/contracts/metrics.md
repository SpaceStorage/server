# Contract: Metrics owned by `009`

**Feature**: `009-admin-ui-ingest` | Exposition: `008` | Increments: this crate

Do **not** increment `spacestorage_log_export_*` (`008`). Ingest vs export (FR-011).

| Series | Labels | Type | When |
|--------|--------|------|------|
| `spacestorage_ingest_records_total` | `source`, `namespace`, `container`, `format`, `result` | counter | each attempt; `result=ok\|parse_error\|dropped` |
| `spacestorage_ingest_parse_errors_total` | `source`, `namespace`, `container`, `reason` | counter | UTF-8, json_root, syslog_unparsed |
| `spacestorage_ingest_dropped_total` | `source`, `reason` | counter | `buffer_full` |
| `spacestorage_ingest_kafka_offset_commits_total` | `namespace`, `container`, `result` | counter | `ok` after durable ack |

`source` = `kafka` \| `syslog`. `format` = `raw` \| `json` \| `rfc5424` \| `rfc3164`. Omit `user` unless a human principal is on the write (ingest uses application `spacestorage-ingest`).

UI queries increment `005` query series with `application="spacestorage-ui"` when that label is known (`008` omit-if-unknown).
