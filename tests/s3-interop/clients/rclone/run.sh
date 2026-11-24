#!/bin/sh
set -eu
: "${S3_ENDPOINT_DOCKER:?}"
: "${S3_BUCKET:?}"
cat > /tmp/rclone.conf <<EOF
[ss]
type = s3
provider = Other
access_key_id = ${S3_ACCESS_KEY_ID}
secret_access_key = ${S3_SECRET_ACCESS_KEY}
endpoint = ${S3_ENDPOINT_DOCKER}
region = ${S3_REGION}
force_path_style = true
acl = private
EOF
printf 'suite-b-rclone\n' > /tmp/fixture.bin
expect=$(sha256sum /tmp/fixture.bin | awk '{print $1}')
rclone --config /tmp/rclone.conf mkdir "ss:${S3_BUCKET}" || true
rclone --config /tmp/rclone.conf copyto /tmp/fixture.bin "ss:${S3_BUCKET}/rclone/fixture.bin"
rclone --config /tmp/rclone.conf ls "ss:${S3_BUCKET}/rclone"
rclone --config /tmp/rclone.conf copyto "ss:${S3_BUCKET}/rclone/fixture.bin" /tmp/out.bin
got=$(sha256sum /tmp/out.bin | awk '{print $1}')
test "$expect" = "$got"
echo "rclone sha256 ok $got"
