# COPY and BEGIN/COMMIT/ROLLBACK are dialect refusals, not config errors.

See [dialect-first-binary.md](../dialect-first-binary.md).

Automated check: `crates/conformance` G4 — issue `BEGIN;`, `COMMIT;`, `ROLLBACK;`, and `COPY t FROM STDIN` on the first-binary PostgreSQL handler; expect SQLSTATE `0A000`. There is no `node.conf` knob that enables them in this profile.
