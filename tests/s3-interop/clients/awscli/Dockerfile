FROM python:3.12-slim
RUN pip install --no-cache-dir 'awscli==1.32.84'
COPY run.sh /run.sh
ENTRYPOINT ["/bin/sh", "/run.sh"]
