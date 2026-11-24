#!/bin/bash
set -euo pipefail
: "${S3_ENDPOINT_DOCKER:?}"
# Strip scheme for clickhouse-backup endpoint (it wants a URL).
mkdir -p /etc/clickhouse-server/config.d /var/lib/clickhouse
cat > /etc/clickhouse-server/config.d/abs-path.xml <<'EOF'
<clickhouse>
  <path>/var/lib/clickhouse/</path>
</clickhouse>
EOF
chown -R clickhouse:clickhouse /var/lib/clickhouse
if [ ! -f /etc/clickhouse-server/users.d/allow.xml ]; then
  mkdir -p /etc/clickhouse-server/users.d
  cat > /etc/clickhouse-server/users.d/allow.xml <<'EOF'
<clickhouse>
  <users>
    <default>
      <password></password>
      <networks><ip>::/0</ip></networks>
      <access_management>1</access_management>
    </default>
  </users>
</clickhouse>
EOF
fi
su -s /bin/bash clickhouse -c 'clickhouse-server --config-file=/etc/clickhouse-server/config.xml --daemon'
ready=0
for i in $(seq 1 60); do
  if clickhouse-client --query 'SELECT 1' >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" != 1 ]; then
  echo "clickhouse-server did not accept connections" >&2
  tail -40 /var/log/clickhouse-server/clickhouse-server.err.log >&2 || true
  exit 1
fi
clickhouse-client --query 'CREATE TABLE IF NOT EXISTS t (id UInt8) ENGINE = MergeTree ORDER BY id'
clickhouse-client --query 'INSERT INTO t VALUES (1)'

hostport="${S3_ENDPOINT_DOCKER#http://}"
hostport="${hostport#https://}"
cat > /tmp/ch-backup.yml <<EOF
general:
  remote_storage: s3
  backups_to_keep_local: 1
  backups_to_keep_remote: 1
clickhouse:
  host: localhost
  port: 9000
  username: default
  password: ""
s3:
  access_key: ${S3_ACCESS_KEY_ID}
  secret_key: ${S3_SECRET_ACCESS_KEY}
  bucket: ${S3_BUCKET}
  endpoint: ${S3_ENDPOINT_DOCKER}
  region: ${S3_REGION}
  force_path_style: true
  disable_ssl: true
  path: clickhouse/
EOF

clickhouse-backup -c /tmp/ch-backup.yml create tiny
clickhouse-backup -c /tmp/ch-backup.yml list local || true
src=$(find /var/lib/clickhouse /var/lib/clickhouse/backup /var/lib/clickhouse/disks -type f -path '*tiny*' 2>/dev/null | head -n 1 || true)
if [ -z "$src" ]; then
  echo "clickhouse-backup: no local files for backup tiny" >&2
  find /var/lib/clickhouse -maxdepth 5 -type d 2>/dev/null | head -120 >&2 || true
  exit 1
fi
expect=$(sha256sum "$src" | awk '{print $1}')
backup_root=$(dirname "$src")
while [ "$(basename "$backup_root")" != "tiny" ] && [ "$backup_root" != "/" ]; do
  backup_root=$(dirname "$backup_root")
done
test "$(basename "$backup_root")" = "tiny"
rel=${src#"$backup_root"/}
clickhouse-backup -c /tmp/ch-backup.yml upload tiny
clickhouse-backup -c /tmp/ch-backup.yml list remote | grep -q tiny
rm -rf "$backup_root"
clickhouse-backup -c /tmp/ch-backup.yml download tiny
got=$(sha256sum "${backup_root}/${rel}" | awk '{print $1}')
test "$expect" = "$got"
echo "clickhouse-backup sha256 ok $got"
