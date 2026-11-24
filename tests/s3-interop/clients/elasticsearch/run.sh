#!/bin/bash
set -euo pipefail
: "${S3_ENDPOINT_DOCKER:?}"
cd /usr/share/elasticsearch
if ! bin/elasticsearch-plugin list | grep -q repository-s3; then
  bin/elasticsearch-plugin install --batch repository-s3
fi
printf '%s' "${S3_ACCESS_KEY_ID}" | bin/elasticsearch-keystore add --stdin --force s3.client.default.access_key
printf '%s' "${S3_SECRET_ACCESS_KEY}" | bin/elasticsearch-keystore add --stdin --force s3.client.default.secret_key
bin/elasticsearch &
esp=$!
for i in $(seq 1 90); do
  if curl -sf http://127.0.0.1:9200 >/dev/null; then
    break
  fi
  sleep 2
done
curl -sf http://127.0.0.1:9200 >/dev/null
hostport="${S3_ENDPOINT_DOCKER#http://}"
curl -sf -X PUT "http://127.0.0.1:9200/_snapshot/ss" \
  -H 'Content-Type: application/json' \
  -d "{\"type\":\"s3\",\"settings\":{\"bucket\":\"${S3_BUCKET}\",\"endpoint\":\"${hostport}\",\"protocol\":\"http\",\"path_style_access\":true,\"base_path\":\"elasticsearch\"}}"
curl -sf -X PUT 'http://127.0.0.1:9200/suiteb_idx' -H 'Content-Type: application/json' -d '{"settings":{"number_of_shards":1,"number_of_replicas":0}}'
curl -sf -X PUT 'http://127.0.0.1:9200/suiteb_idx/_doc/1' -H 'Content-Type: application/json' -d '{"ok":true}'
curl -sf -X POST 'http://127.0.0.1:9200/suiteb_idx/_refresh'
snap_name="snap$(date +%s)"
snap=$(curl -sf -X PUT "http://127.0.0.1:9200/_snapshot/ss/${snap_name}?wait_for_completion=true")
echo "$snap" | grep -q SUCCESS
python3 - <<'PY'
import hashlib, os
import boto3
from botocore.config import Config
s3 = boto3.client(
    "s3",
    endpoint_url=os.environ["S3_ENDPOINT_DOCKER"],
    aws_access_key_id=os.environ["S3_ACCESS_KEY_ID"],
    aws_secret_access_key=os.environ["S3_SECRET_ACCESS_KEY"],
    region_name=os.environ.get("S3_REGION", "us-east-1"),
    config=Config(signature_version="s3v4", s3={"addressing_style": "path"}),
)
listed = s3.list_objects_v2(Bucket=os.environ["S3_BUCKET"], Prefix="elasticsearch/")
contents = listed.get("Contents") or []
if not contents:
    raise SystemExit("elasticsearch snapshot stored no objects")
key = contents[0]["Key"]
body = s3.get_object(Bucket=os.environ["S3_BUCKET"], Key=key)["Body"].read()
again = s3.get_object(Bucket=os.environ["S3_BUCKET"], Key=key)["Body"].read()
digest = hashlib.sha256(body).hexdigest()
assert hashlib.sha256(again).hexdigest() == digest
print("elasticsearch sha256 ok", key, digest)
PY
kill "$esp" || true
wait "$esp" || true
