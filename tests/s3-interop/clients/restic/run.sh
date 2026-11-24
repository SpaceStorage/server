#!/bin/sh
set -eu
: "${S3_ENDPOINT_DOCKER:?}"
export RESTIC_PASSWORD=suiteb-restic
export RESTIC_REPOSITORY="s3:${S3_ENDPOINT_DOCKER}/${S3_BUCKET}/restic"
export AWS_ACCESS_KEY_ID="${S3_ACCESS_KEY_ID}"
export AWS_SECRET_ACCESS_KEY="${S3_SECRET_ACCESS_KEY}"
export AWS_DEFAULT_REGION="${S3_REGION}"
export AWS_EC2_METADATA_DISABLED=true
printf 'suite-b-restic\n' > /tmp/fixture.txt
expect=$(sha256sum /tmp/fixture.txt | awk '{print $1}')
if ! restic cat config >/dev/null 2>&1; then
  restic init
fi
restic backup /tmp/fixture.txt
restic check
mkdir -p /restore
restic restore latest --target /restore
got=$(sha256sum /restore/tmp/fixture.txt | awk '{print $1}')
test "$expect" = "$got"
echo "restic sha256 ok $got"
