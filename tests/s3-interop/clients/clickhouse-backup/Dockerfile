FROM clickhouse/clickhouse-server:24.8
USER root
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && arch="$(dpkg --print-architecture)" \
 && case "$arch" in amd64) b=amd64 ;; arm64) b=arm64 ;; *) echo "unsupported $arch"; exit 1 ;; esac \
 && curl -fsSL -o /tmp/clickhouse-backup.tgz \
      "https://github.com/Altinity/clickhouse-backup/releases/download/v2.6.5/clickhouse-backup-linux-${b}.tar.gz" \
 && tar -xzf /tmp/clickhouse-backup.tgz -C /tmp \
 && bin="$(find /tmp -type f -name clickhouse-backup | head -n 1)" \
 && test -n "$bin" \
 && install -m 755 "$bin" /usr/local/bin/clickhouse-backup \
 && rm -rf /tmp/clickhouse-backup.tgz /tmp/build /tmp/clickhouse-backup* \
 && rm -rf /var/lib/apt/lists/*
COPY run.sh /run.sh
ENTRYPOINT ["/bin/bash", "/run.sh"]
