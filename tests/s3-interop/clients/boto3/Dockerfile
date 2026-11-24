FROM python:3.12-slim
RUN pip install --no-cache-dir 'boto3==1.34.162' requests
COPY run.py /run.py
ENTRYPOINT ["python", "/run.py"]
