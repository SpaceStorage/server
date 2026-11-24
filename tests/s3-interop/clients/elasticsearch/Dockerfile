FROM docker.elastic.co/elasticsearch/elasticsearch:7.17.27
USER root
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 python3-boto3 ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY run.sh /run.sh
RUN chmod 755 /run.sh
USER elasticsearch
ENTRYPOINT ["/bin/bash", "/run.sh"]
