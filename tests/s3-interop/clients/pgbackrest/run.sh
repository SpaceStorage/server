#!/bin/bash
set -euo pipefail
: "${S3_ENDPOINT_DOCKER:?}"
hostport="${S3_ENDPOINT_DOCKER#http://}"
host="${hostport%%:*}"
port="${hostport##*:}"
# pgBackRest always speaks TLS to S3. Terminate TLS locally and forward plaintext.
openssl req -x509 -newkey rsa:2048 -keyout /tmp/pgb.key -out /tmp/pgb.crt -days 1 -nodes -subj /CN=127.0.0.1 >/dev/null 2>&1
cat > /tmp/tls_proxy.py <<'PY'
import os, socket, ssl, threading
backend = (os.environ["PG_BACKEND_HOST"], int(os.environ["PG_BACKEND_PORT"]))
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain("/tmp/pgb.crt", "/tmp/pgb.key")

def pipe(src, dst):
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
    except Exception:
        pass

def handle(conn):
    upstream = None
    tls = None
    try:
        tls = ctx.wrap_socket(conn, server_side=True)
        upstream = socket.create_connection(backend, timeout=30)
        t1 = threading.Thread(target=pipe, args=(tls, upstream), daemon=True)
        t2 = threading.Thread(target=pipe, args=(upstream, tls), daemon=True)
        t1.start()
        t2.start()
        t1.join()
        t2.join()
    except Exception as exc:
        print("tls-proxy", exc, flush=True)
    finally:
        for sock in (tls, upstream, conn):
            if sock is not None:
                try:
                    sock.close()
                except Exception:
                    pass

srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", 9443))
srv.listen(64)
while True:
    client, _ = srv.accept()
    threading.Thread(target=handle, args=(client,), daemon=True).start()
PY
export PG_BACKEND_HOST="$host" PG_BACKEND_PORT="$port"
python3 /tmp/tls_proxy.py >/tmp/tls-proxy.log 2>&1 &
ready=0
for _ in $(seq 1 50); do
  if python3 -c 'import socket; s=socket.create_connection(("127.0.0.1",9443),1); s.close()' >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.1
done
if [ "$ready" != 1 ]; then
  echo "pgbackrest tls proxy failed to listen" >&2
  cat /tmp/tls-proxy.log >&2 || true
  exit 1
fi
export PGB_PREFIX="pgbackrest/$(date +%s)-$$"
mkdir -p /etc/pgbackrest /var/log/pgbackrest /var/lib/pgbackrest
chown -R postgres:postgres /var/log/pgbackrest /var/lib/pgbackrest /etc/pgbackrest
cat > /etc/pgbackrest/pgbackrest.conf <<EOF
[global]
repo1-type=s3
repo1-path=/${PGB_PREFIX}
repo1-s3-endpoint=127.0.0.1
repo1-s3-port=9443
repo1-s3-bucket=${S3_BUCKET}
repo1-s3-region=${S3_REGION}
repo1-s3-key=${S3_ACCESS_KEY_ID}
repo1-s3-key-secret=${S3_SECRET_ACCESS_KEY}
repo1-s3-uri-style=path
repo1-s3-verify-tls=n
repo1-retention-full=1
start-fast=y
log-level-console=info

[demo]
pg1-path=/var/lib/postgresql/data
pg1-port=5432
pg1-user=postgres
EOF
chmod 640 /etc/pgbackrest/pgbackrest.conf
chown postgres:postgres /etc/pgbackrest/pgbackrest.conf

if [ ! -f /var/lib/postgresql/data/PG_VERSION ]; then
  su postgres -c "initdb -D /var/lib/postgresql/data"
fi
cat >> /var/lib/postgresql/data/postgresql.conf <<'EOF'
archive_mode = on
archive_command = 'pgbackrest --stanza=demo archive-push %p'
wal_level = replica
max_wal_senders = 3
listen_addresses = '127.0.0.1'
EOF
su postgres -c "pg_ctl -D /var/lib/postgresql/data -l /tmp/pg.log start"
for i in $(seq 1 30); do
  if su postgres -c "psql -c 'SELECT 1'" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
su postgres -c "psql -c 'CREATE TABLE IF NOT EXISTS t(id int); INSERT INTO t VALUES (1);'"
su postgres -c "pgbackrest --stanza=demo stanza-create"
su postgres -c "pgbackrest --stanza=demo check"
su postgres -c "pgbackrest --stanza=demo backup --type=full"
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
listed = s3.list_objects_v2(Bucket=os.environ["S3_BUCKET"], Prefix=os.environ["PGB_PREFIX"] + "/")
contents = listed.get("Contents") or []
if not contents:
    raise SystemExit("pgbackrest stored no objects")
key = contents[0]["Key"]
body = s3.get_object(Bucket=os.environ["S3_BUCKET"], Key=key)["Body"].read()
again = s3.get_object(Bucket=os.environ["S3_BUCKET"], Key=key)["Body"].read()
digest = hashlib.sha256(body).hexdigest()
assert hashlib.sha256(again).hexdigest() == digest and len(body) > 0
print("pgbackrest sha256 ok", key, digest, len(body))
PY
