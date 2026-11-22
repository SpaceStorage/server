#!/bin/sh
# Start spacestoraged for the Compose three-node lab.
# Bootstrap (db-a) creates shared secrets. Joiners wait for ready, mint a
# one-time join token against the seed, then start.
#
# Lab joiners always wipe local identity/raft and remint a token. Persisted
# joiner identity would skip seed contact (restart-as-member), while the seed
# rebuilds a one-node view after restart — so the cluster map would stay at 1.
set -eu

NODE_ROLE="${NODE_ROLE:?NODE_ROLE required (bootstrap|join)}"
NODE_NAME="${NODE_NAME:?NODE_NAME required}"
CONFIG="${CONFIG:?CONFIG required}"
SECRETS="${SECRETS_DIR:-/etc/spacestorage/secrets}"
DATA="${DATA_DIR:-/var/lib/spacestorage}"
SEED_HTTP="${SEED_HTTP:-http://db-a:7701}"

mkdir -p "$SECRETS" "$DATA/nvme0"

ensure_secrets() {
  if [ -f "$SECRETS/admin.token" ] \
    && [ -f "$SECRETS/master.key" ] \
    && [ -f "$SECRETS/join.secret" ]; then
    return 0
  fi
  # Only bootstrap creates secrets; joiners wait.
  if [ "$NODE_ROLE" != "bootstrap" ]; then
    return 1
  fi
  umask 077
  # Drop a stale lock from a killed create, then take the lock.
  rmdir "$SECRETS/.creating" 2>/dev/null || true
  if mkdir "$SECRETS/.creating" 2>/dev/null; then
    head -c 32 /dev/urandom | base64 | tr -d '\n' >"$SECRETS/admin.token"
    head -c 32 /dev/urandom >"$SECRETS/master.key"
    head -c 32 /dev/urandom >"$SECRETS/join.secret"
    chmod 600 "$SECRETS/admin.token" "$SECRETS/master.key" "$SECRETS/join.secret"
    rmdir "$SECRETS/.creating"
  fi
  return 0
}

wait_for_file() {
  path="$1"
  tries="${2:-90}"
  i=0
  while [ ! -f "$path" ]; do
    i=$((i + 1))
    if [ "$i" -gt "$tries" ]; then
      echo "timeout waiting for $path" >&2
      exit 1
    fi
    sleep 1
  done
}

wait_for_ready() {
  url="$1/v1/health/ready"
  tries="${2:-180}"
  i=0
  while ! curl -sf "$url" >/dev/null 2>&1; do
    i=$((i + 1))
    if [ "$i" -gt "$tries" ]; then
      echo "timeout waiting for ready at $url" >&2
      exit 1
    fi
    sleep 1
  done
}

prepare_joiner() {
  # Force first-join path every start (see file header).
  rm -rf "$DATA/identity" "$DATA/raft"
  rm -f "$SECRETS/${NODE_NAME}.join.token"
  mkdir -p "$DATA/nvme0"
}

mint_join_token() {
  token_file="$SECRETS/${NODE_NAME}.join.token"
  export SPACESTORAGE_TOKEN
  SPACESTORAGE_TOKEN="$(cat "$SECRETS/admin.token")"
  out="$(spacestorage join-token mint --name "$NODE_NAME" --endpoint "$SEED_HTTP" --output json)"
  token="$(printf '%s' "$out" | sed -n 's/.*"token"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
  if [ -z "$token" ]; then
    echo "failed to mint join token for $NODE_NAME: $out" >&2
    exit 1
  fi
  printf '%s\n' "$token" >"$token_file"
  chmod 600 "$token_file"
  echo "minted join token for $NODE_NAME"
}

if [ "$NODE_ROLE" = "bootstrap" ]; then
  ensure_secrets
  wait_for_file "$SECRETS/join.secret" 30
else
  wait_for_file "$SECRETS/join.secret" 120
  wait_for_ready "$SEED_HTTP" 180
  prepare_joiner
  mint_join_token
fi

echo "starting spacestoraged role=$NODE_ROLE name=$NODE_NAME config=$CONFIG"
exec spacestoraged --config "$CONFIG"
