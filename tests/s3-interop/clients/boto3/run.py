#!/usr/bin/env python3
"""boto3 Suite B row: path-style, optional vhost, presign, multipart, metadata, range, unsupported code."""
import hashlib
import io
import os
import sys

import boto3
import requests
from botocore.client import Config
from botocore.exceptions import ClientError
from boto3.s3.transfer import TransferConfig

endpoint = os.environ["S3_ENDPOINT_DOCKER"]
region = os.environ.get("S3_REGION", "us-east-1")
bucket = os.environ["S3_BUCKET"]
key_id = os.environ["S3_ACCESS_KEY_ID"]
secret = os.environ["S3_SECRET_ACCESS_KEY"]
vhost = os.environ.get("S3_VHOST_SUFFIX", "").strip()


def client(url, addressing):
    return boto3.client(
        "s3",
        endpoint_url=url,
        aws_access_key_id=key_id,
        aws_secret_access_key=secret,
        region_name=region,
        config=Config(signature_version="s3v4", s3={"addressing_style": addressing}),
    )


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def ensure_bucket(s3):
    try:
        s3.create_bucket(Bucket=bucket)
    except ClientError as exc:
        code = exc.response.get("Error", {}).get("Code", "")
        if code not in ("BucketAlreadyOwnedByYou", "BucketAlreadyExists"):
            raise


s3 = client(endpoint, "path")
ensure_bucket(s3)

body = b"suite-b-boto3\n"
s3.put_object(Bucket=bucket, Key="boto3/obj", Body=body, Metadata={"color": "blue"})
got = s3.get_object(Bucket=bucket, Key="boto3/obj")
raw = got["Body"].read()
assert sha256(raw) == sha256(body), "path-style sha256"
head = s3.head_object(Bucket=bucket, Key="boto3/obj")
meta = {k.lower(): v for k, v in head.get("Metadata", {}).items()}
assert meta.get("color") == "blue", head

ranged = s3.get_object(Bucket=bucket, Key="boto3/obj", Range="bytes=0-3")["Body"].read()
assert ranged == body[:4], ranged

url = s3.generate_presigned_url(
    "get_object", Params={"Bucket": bucket, "Key": "boto3/obj"}, ExpiresIn=120
)
resp = requests.get(url, timeout=30)
assert resp.status_code == 200, (resp.status_code, resp.text[:300])
assert sha256(resp.content) == sha256(body), "presign sha256"

payload = b"P" * (5 * 1024 * 1024) + b"Q" * 16
s3.upload_fileobj(
    io.BytesIO(payload),
    bucket,
    "boto3/mp",
    Config=TransferConfig(multipart_threshold=5 * 1024 * 1024, multipart_chunksize=5 * 1024 * 1024),
)
mp = s3.get_object(Bucket=bucket, Key="boto3/mp")["Body"].read()
assert sha256(mp) == sha256(payload), "multipart sha256"

try:
    s3.put_bucket_versioning(
        Bucket=bucket, VersioningConfiguration={"Status": "Enabled"}
    )
    sys.exit("versioning returned success")
except ClientError as exc:
    code = exc.response.get("Error", {}).get("Code", "")
    if code not in ("NotImplemented", "compat_must_not"):
        sys.exit(f"unexpected unsupported code {code}")
    print("unsupported", code)

if vhost:
    vclient = client(f"http://{vhost}:19001" if "://" not in vhost else endpoint, "virtual")
    # endpoint host is the suffix; boto3 virtual-host prepends the bucket.
    if "://" not in vhost:
        port = endpoint.rsplit(":", 1)[-1]
        vclient = client(f"http://{vhost}:{port}", "virtual")
    vbody = b"vhost-boto3"
    vclient.put_object(Bucket=bucket, Key="boto3/vhost", Body=vbody)
    vgot = vclient.get_object(Bucket=bucket, Key="boto3/vhost")["Body"].read()
    assert sha256(vgot) == sha256(vbody), "vhost sha256"
    print("boto3 vhost ok")

print("boto3 sha256 ok", sha256(body))
