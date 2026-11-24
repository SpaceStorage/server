FROM golang:1.23-bookworm
WORKDIR /src
COPY go.mod main.go ./
ENV GOTOOLCHAIN=auto
RUN go mod tidy && go build -o /sdk .
ENTRYPOINT ["/sdk"]
