#!/bin/sh
# Start the in-process S3 listener (demo/demo) and run Suite B against it.
set -eu
SERVER=$(CDPATH= cd -- "$(dirname "$0")/../../.." && pwd)
cd "$SERVER"

export S3_LISTEN="${S3_LISTEN:-0.0.0.0:19001}"
export S3_VHOST_SUFFIX="${S3_VHOST_SUFFIX:-s3.local}"
export S3_MAX_VALUE="${S3_MAX_VALUE:-1GiB}"
export S3_MAX_KEY="${S3_MAX_KEY:-4KiB}"
port=${S3_LISTEN##*:}

if ! nc -z 127.0.0.1 "$port" 2>/dev/null; then
  echo "starting s3_listen on $S3_LISTEN"
  cargo run -q -p spacestorage-handler-s3 --example s3_listen >"$SERVER/target/s3_listen.log" 2>&1 &
  echo $! >"$SERVER/target/s3_listen.pid"
  i=0
  while [ "$i" -lt 120 ]; do
    if nc -z 127.0.0.1 "$port" 2>/dev/null; then
      break
    fi
    i=$((i + 1))
    sleep 1
  done
  if ! nc -z 127.0.0.1 "$port" 2>/dev/null; then
    echo "s3_listen did not accept on $port" >&2
    tail -n 50 "$SERVER/target/s3_listen.log" >&2 || true
    exit 1
  fi
fi

export S3_ENDPOINT="http://127.0.0.1:${port}"
export S3_ACCESS_KEY_ID=demo
export S3_SECRET_ACCESS_KEY=demo
export S3_REGION=us-east-1
export S3_BUCKET=suiteb
exec "$SERVER/tests/s3-interop/harness/run.sh"
