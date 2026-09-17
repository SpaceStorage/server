# Contract: CLI

**Feature**: `010-migration-transforms` | Binary: `spacestorage` (`001`)

```text
spacestorage job list
spacestorage job status <id>
spacestorage job cancel <id>
spacestorage job resume <id>

spacestorage migrate --from <ns.container> [--to-node <id>] [--to-namespace <ns>] [--name <dest>] [--policy copy|move] [--strategy live|snapshot|offline]
spacestorage transform --from <ns.container> --rewrite <kind> [--to-type <type>] [--mapping-file <json>] [--swap|--no-swap] [--retain-source]
spacestorage backup --namespace <ns> | --container <ns.container> [--pitr]
spacestorage restore --snapshot <id> [--pitr <pos>] [--namespace <ns>] [--key-ref <ref>]
```

Exit codes: `001` conventions. Named job errors print `code` + message on stderr; exit 2 for validation, 3 for authz, 5 for slice-10 required.

`--mapping-file` is MappingQuery JSON ([mapping.md](mapping.md)), not SQL.
