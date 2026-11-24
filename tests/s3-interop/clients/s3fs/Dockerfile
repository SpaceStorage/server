FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends s3fs python3 python3-boto3 ca-certificates fuse \
 && rm -rf /var/lib/apt/lists/*
COPY run.sh /run.sh
ENTRYPOINT ["/bin/bash", "/run.sh"]
