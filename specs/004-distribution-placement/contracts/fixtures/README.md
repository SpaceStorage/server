# Fixtures: Distribution and Placement

Starter topologies for FR-075 and invalid cases for validate-phase codes.

| Path | Purpose |
|------|---------|
| `node-single.conf` | Single node, internodes disabled |
| `node-rack.conf` | Three processes would share this shape with different names; one-file rack example |
| `cluster-3az/db-*.conf` | Three AZ, one region, internodes meshed |
| `cluster-mixed-media/` | NVMe vs HDD |
| `cluster-two-region/` | Sync EU, async US (timing + optional delay) |
| `invalid/` | One file per new validation code |

Each starter must pass `spacestorage validate` unchanged. The two-region files include `replication` knobs; long-RTT is the same files with `failure_timeout 30m` as documented in quickstart (operators edit, or use `--set`).
