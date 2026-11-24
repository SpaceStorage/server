#!/bin/sh
# T060: fail fast when endpoint unset (SC-003).
set -eu
if [ -z "${S3_ENDPOINT:-}" ]; then
  echo "S3_ENDPOINT unset" >&2
  exit 1
fi
echo "S3_ENDPOINT=$S3_ENDPOINT"
exit 0