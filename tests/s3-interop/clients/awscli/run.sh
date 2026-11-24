#!/bin/sh
set -eu
: "${S3_ENDPOINT_DOCKER:?}"
EP="$S3_ENDPOINT_DOCKER"
B="$S3_BUCKET"
aws --endpoint-url "$EP" s3api create-bucket --bucket "$B" >/dev/null 2>&1 || true
printf 'suite-b-aws\n' > /tmp/aws-obj.bin
aws --endpoint-url "$EP" s3 cp /tmp/aws-obj.bin "s3://${B}/aws/obj.bin"
aws --endpoint-url "$EP" s3api head-object --bucket "$B" --key aws/obj.bin
aws --endpoint-url "$EP" s3 cp "s3://${B}/aws/obj.bin" /tmp/aws-out.bin
aws --endpoint-url "$EP" s3 cp "s3://${B}/aws/obj.bin" "s3://${B}/aws/obj-copy.bin"
aws --endpoint-url "$EP" s3 rm "s3://${B}/aws/obj-copy.bin"
python3 - <<'PY'
import hashlib, pathlib
a = pathlib.Path("/tmp/aws-obj.bin").read_bytes()
b = pathlib.Path("/tmp/aws-out.bin").read_bytes()
assert hashlib.sha256(a).digest() == hashlib.sha256(b).digest()
print("awscli put/get sha256 ok", hashlib.sha256(b).hexdigest())
PY
python3 - <<'PY'
from pathlib import Path
Path("/tmp/aws-big.bin").write_bytes(b"M" * (5 * 1024 * 1024) + b"Z" * 32)
PY
aws configure set default.s3.multipart_threshold 5242880
aws configure set default.s3.multipart_chunksize 5242880
aws --endpoint-url "$EP" s3 cp /tmp/aws-big.bin "s3://${B}/aws/big.bin"
aws --endpoint-url "$EP" s3 cp "s3://${B}/aws/big.bin" /tmp/aws-big-out.bin
python3 - <<'PY'
import hashlib, pathlib
a = pathlib.Path("/tmp/aws-big.bin").read_bytes()
b = pathlib.Path("/tmp/aws-big-out.bin").read_bytes()
assert hashlib.sha256(a).digest() == hashlib.sha256(b).digest(), (len(a), len(b))
print("awscli multipart sha256 ok", len(b))
PY
