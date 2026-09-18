# Fixtures: internodes, replication, domains

Configs must pass `spacestorage validate`. Loopback + `plaintext;` for conformance. Real clusters bind a non-loopback cluster address and usually `tls`.

| File | Purpose |
|------|---------|
| [single-node.conf](single-node.conf) | Bootstrap, `default` domain, both handlers on loopback |
| [three-node-a.conf](three-node-a.conf) | Member in `default`; cluster address for join |
| [two-domain-source.conf](two-domain-source.conf) | Source domain `eu` (example names; objects, not labels) |
| [invalid/omit-transport.conf](invalid/omit-transport.conf) | Startup fail |
| [invalid/omit-internodes.conf](invalid/omit-internodes.conf) | Startup fail |
| [invalid/loopback-join.conf](invalid/loopback-join.conf) | Join remote refused |

Shared with `011`/`004` starters: `cluster { token_file; }`, ladder `[az]`, admin declarations.
