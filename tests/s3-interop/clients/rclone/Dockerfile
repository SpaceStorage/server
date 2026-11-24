FROM alpine:3.20
RUN apk add --no-cache rclone ca-certificates
COPY run.sh /run.sh
ENTRYPOINT ["/bin/sh", "/run.sh"]
