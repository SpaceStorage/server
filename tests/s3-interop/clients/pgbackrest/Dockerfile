FROM postgres:16-bookworm
RUN apt-get update \
 && apt-get install -y --no-install-recommends pgbackrest python3 python3-boto3 ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY run.sh /run.sh
ENTRYPOINT ["/bin/bash", "/run.sh"]
