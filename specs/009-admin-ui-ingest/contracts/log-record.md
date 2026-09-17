# Contract: Stored log record and payload formats

**Feature**: `009-admin-ui-ingest` | Append: `003` `log_stream.append`

Field table: [data-model.md §4](../data-model.md).

## Kafka `format raw` (default)

- Value MUST be UTF-8. Else parse failure.
- `message` = value; `kafka_key` = key (UTF-8 lossy); `timestamp` = broker timestamp or ingest now; topic/partition/offset sidecar.

## Kafka `format json`

- Value MUST be a JSON **object**. Arrays, numbers, strings at the root → parse failure.
- Known keys map to `timestamp`, `message`, `severity`, `host`, `app_name`, `procid`, `msgid`, `facility`.
- Other keys → `attrs` object.
- Sidecar Kafka fields still stored.

## Syslog

RFC 5424 fields as named. RFC 3164: timestamp (or ingest now if unparsable date), host, tag → `app_name`, remainder → `message`. PRI → `facility` + `severity`.

## Skip policy

Parse failures increment `spacestorage_ingest_parse_errors_total` and MUST NOT stall the partition or syslog listener (FR-010, SC-004, SC-005). Dead-letter container is out of this feature.
