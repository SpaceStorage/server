#!/bin/bash
set -euo pipefail
: "${S3_ENDPOINT_DOCKER:?}"
if [ ! -e /dev/fuse ]; then
  echo "s3fs: /dev/fuse is not available in this container" >&2
  exit 1
fi
echo "${S3_ACCESS_KEY_ID}:${S3_SECRET_ACCESS_KEY}" > /etc/passwd-s3fs
chmod 600 /etc/passwd-s3fs
grep -q '^user_allow_other' /etc/fuse.conf 2>/dev/null || echo user_allow_other >> /etc/fuse.conf
python3 - <<'PY'
import os
import boto3
from botocore.config import Config
from botocore.exceptions import ClientError
s3 = boto3.client(
    "s3",
    endpoint_url=os.environ["S3_ENDPOINT_DOCKER"],
    aws_access_key_id=os.environ["S3_ACCESS_KEY_ID"],
    aws_secret_access_key=os.environ["S3_SECRET_ACCESS_KEY"],
    region_name=os.environ.get("S3_REGION", "us-east-1"),
    config=Config(signature_version="s3v4", s3={"addressing_style": "path"}),
)
try:
    s3.create_bucket(Bucket=os.environ["S3_BUCKET"])
except ClientError as exc:
    code = exc.response.get("Error", {}).get("Code", "")
    if code not in ("BucketAlreadyOwnedByYou", "BucketAlreadyExists"):
        raise
PY
mkdir -p /mnt/s3
s3fs "${S3_BUCKET}" /mnt/s3 \
  -o passwd_file=/etc/passwd-s3fs \
  -o "url=${S3_ENDPOINT_DOCKER}" \
  -o use_path_request_style \
  -o allow_other \
  -o nonempty \
  -o umask=0000
printf 'suite-b-s3fs\n' > /mnt/s3/s3fs-obj
sync
python3 - <<'PY'
import hashlib, os
import boto3
from botocore.config import Config
body = b"suite-b-s3fs\n"
s3 = boto3.client(
    "s3",
    endpoint_url=os.environ["S3_ENDPOINT_DOCKER"],
    aws_access_key_id=os.environ["S3_ACCESS_KEY_ID"],
    aws_secret_access_key=os.environ["S3_SECRET_ACCESS_KEY"],
    region_name=os.environ.get("S3_REGION", "us-east-1"),
    config=Config(signature_version="s3v4", s3={"addressing_style": "path"}),
)
got = s3.get_object(Bucket=os.environ["S3_BUCKET"], Key="s3fs-obj")["Body"].read()
assert hashlib.sha256(got).digest() == hashlib.sha256(body).digest(), got
print("s3fs sha256 ok", hashlib.sha256(got).hexdigest())
PY
fusermount -u /mnt/s3 || umount /mnt/s3
