# Contract: PostgreSQL COPY

**Feature**: `015-compatibility-and-limits` | Spec: FR-008 | Exec: `005` `CopyFormat`

| Profile | COPY |
|---------|------|
| FirstBinary, HandlersComplete | MUST NOT (`copy_not_in_profile`) |
| CompleteProduct | MUST `text`, `csv`, `binary` inbound and outbound on a `Relational Table` |

MUST NOT in every profile: `COPY … PROGRAM`, `FREEZE`, COPY to/from a server path (stdin/stdout / client COPY protocol only).
