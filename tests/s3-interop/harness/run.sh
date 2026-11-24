#!/bin/sh
# T062/T072: run every Suite B row. Non-zero if any row fails or the endpoint is unset.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"

if [ -z "${S3_ENDPOINT:-}" ]; then
  echo "S3_ENDPOINT unset" >&2
  exit 1
fi

export S3_ACCESS_KEY_ID="${S3_ACCESS_KEY_ID:-demo}"
export S3_SECRET_ACCESS_KEY="${S3_SECRET_ACCESS_KEY:-demo}"
export S3_REGION="${S3_REGION:-us-east-1}"
export S3_BUCKET="${S3_BUCKET:-suiteb}"
export S3_VHOST_SUFFIX="${S3_VHOST_SUFFIX:-s3.local}"

case "$S3_ENDPOINT" in
  http://127.0.0.1:*|http://localhost:*|http://[::1]:*)
    port=${S3_ENDPOINT##*:}
    port=${port%%/*}
    export S3_ENDPOINT_DOCKER="http://host.docker.internal:${port}"
    ;;
  *)
    export S3_ENDPOINT_DOCKER="$S3_ENDPOINT"
    ;;
esac

mkdir -p matrix
rows="rclone awscli boto3 s3fs clickhouse-backup elasticsearch pgbackrest restic aws-sdk-go-v2"
fail=0
json="$ROOT/matrix/results.json"
tmp="$ROOT/matrix/results.json.tmp"
printf '%s\n' '{"rows":[' > "$tmp"
first=1

for row in $rows; do
  echo "=== Suite B: $row ==="
  log="$ROOT/matrix/${row}.log"
  set +e
  docker compose run --rm --build "$row" >"$log" 2>&1
  code=$?
  set -e
  if [ "$code" -eq 0 ]; then
    pass=true
    echo "PASS $row"
  else
    pass=false
    fail=1
    echo "FAIL $row (exit $code)" >&2
    tail -n 40 "$log" >&2 || true
  fi
  # Escape for JSON string: backslash, quotes, control chars.
  detail=$(tail -n 5 "$log" | tr '\n\r\t' '   ' | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read())[1:-1])')
  if [ "$first" -eq 0 ]; then
    printf ',\n' >> "$tmp"
  fi
  first=0
  printf '  {"id":"%s","pass":%s,"exit":%s,"detail":"%s"}' "$row" "$pass" "$code" "$detail" >> "$tmp"
done

printf '\n]}\n' >> "$tmp"
mv "$tmp" "$json"
echo "wrote $json"
exit "$fail"
