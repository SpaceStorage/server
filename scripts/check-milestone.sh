#!/usr/bin/env bash
# Fail CI when a 1–5 milestone omits deferred slices 6–11 (SC-004).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec cargo test -p spacestorage-release-profile --test ledger -- --nocapture
