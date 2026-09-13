# Quickstart: connect stock clients to one SpaceStorage node over every protocol

**Feature**: `002-protocol-drivers` | Proves SC-001, SC-002, SC-003, SC-004, SC-005, SC-006, SC-008, SC-009, SC-010, SC-011 (SC-007 needs a 2-node cluster — see §9)

Contracts referenced: [config directives](contracts/config-directives.md), [query options](contracts/query-options.md), [canonical representation](contracts/canonical-representation.md), per-protocol contracts under [contracts/protocols/](contracts/protocols/), [execution boundary](contracts/execution-boundary.md).

## Prerequisites

- Feature `001` quickstart completed (server + CLI built, `/etc/spacestorage/admin.token`).
- Client tools (any subset is fine; each section is independent): `psql`, `cqlsh`, `redis-cli`, `curl` + `jq`, `clickhouse-client`, `aws` CLI, `cadaver`.
- Ports 5432, 9042, 6379, 9200, 9000, 8123, 9010, 9020 free on loopback.

## 1. Users file and configuration (SC-010)

```bash
sudo install -m 0600 specs/002-protocol-drivers/contracts/fixtures/users.example /etc/spacestorage/users
sudo cp specs/002-protocol-drivers/contracts/fixtures/node-all-protocols.conf /etc/spacestorage/node.conf
spacestorage validate /etc/spacestorage/node.conf           # expected: OK, effective config lists 8 protocol handlers + query_defaults
spacestorage validate specs/002-protocol-drivers/contracts/fixtures/invalid/two-handlers-one-port.conf; echo "exit=$?"   # entrypoint_duplicate_address, exit=2 (SC-008 second half)
spacestoraged --config /etc/spacestorage/node.conf &
sleep 1; spacestorage protocols                               # table: handler, address:port, transport, versions, connections
```

Credentials used below: `app-a / s3cr3t-a` bound to namespace `tenant-a`, `app-b / s3cr3t-b` bound to `tenant-b`.

## 2. Smoke workflow per protocol (SC-001)

Each block: connect → create container → write → read → delete, with the unmodified client.

```bash
# PostgreSQL
PGPASSWORD=s3cr3t-a psql -h 127.0.0.1 -p 5432 -U app-a -d tenant-a -c "CREATE TABLE t (id int PRIMARY KEY, v text);" \
  -c "INSERT INTO t VALUES (1,'a');" -c "SELECT * FROM t;" -c "DROP TABLE t;"

# Cassandra
cqlsh 127.0.0.1 9042 -u app-a -p s3cr3t-a -e "CREATE TABLE tenant_a.t (id int PRIMARY KEY, v text); INSERT INTO tenant_a.t (id,v) VALUES (1,'a'); SELECT * FROM tenant_a.t; DROP TABLE tenant_a.t;"

# Redis
redis-cli -p 6379 --user app-a --pass s3cr3t-a SET k v
redis-cli -p 6379 --user app-a --pass s3cr3t-a GET k          # "v"
redis-cli -p 6379 --user app-a --pass s3cr3t-a DEL k

# Elasticsearch
curl -su app-a:s3cr3t-a -XPUT localhost:9200/docs
curl -su app-a:s3cr3t-a -XPUT localhost:9200/docs/_doc/1 -H 'Content-Type: application/json' -d '{"v":"a"}'
curl -su app-a:s3cr3t-a 'localhost:9200/docs/_search?q=v:a' | jq '.hits.total.value'   # 1
curl -su app-a:s3cr3t-a -XDELETE localhost:9200/docs

# ClickHouse native and HTTP
clickhouse-client --host 127.0.0.1 --port 9000 --user app-a --password s3cr3t-a --database tenant-a \
  -q "CREATE TABLE t (id UInt32, v String) ENGINE = MergeTree ORDER BY id; INSERT INTO t VALUES (1,'a'); SELECT * FROM t; DROP TABLE t;"
echo "SELECT version()" | curl -s 'http://localhost:8123/?database=tenant-a' -u app-a:s3cr3t-a --data-binary @-

# S3 (path-style, any region)
export AWS_ACCESS_KEY_ID=app-a AWS_SECRET_ACCESS_KEY=s3cr3t-a AWS_DEFAULT_REGION=us-east-1
aws --endpoint-url http://127.0.0.1:9010 s3 mb s3://data
head -c 20971520 /dev/urandom > /tmp/big.bin                  # 20 MiB → multipart upload
aws --endpoint-url http://127.0.0.1:9010 s3 cp /tmp/big.bin s3://data/big.bin
aws --endpoint-url http://127.0.0.1:9010 s3 cp s3://data/big.bin /tmp/big.out && cmp /tmp/big.bin /tmp/big.out && echo "round-trip ok"
aws --endpoint-url http://127.0.0.1:9010 s3 rb s3://data --force

# WebDAV
cadaver http://127.0.0.1:9020/ <<'EOF'
mkcol files
put /etc/hostname files/hostname
ls files
move files/hostname files/hostname2
delete files/hostname2
rmcol files
quit
EOF
```

Expected: every command succeeds; `spacestorage executions --limit 20` shows one record per operation with `protocol`, `namespace=tenant-a`, applied `quorum`/`timeout`.

## 3. Full type system through any protocol (SC-002, SC-003)

Create a **vector collection from PostgreSQL**, write to it from Redis using the canonical representation, read it from S3, search it from Elasticsearch, drop it from ClickHouse:

```bash
PGPASSWORD=s3cr3t-a psql -h 127.0.0.1 -U app-a -d tenant-a -c \
  "CREATE TABLE vectors () WITH (spacestorage_type = 'vector_collection', spacestorage_opt_dims = '3');"

redis-cli --user app-a --pass s3cr3t-a SS.CONTAINERS                          # lists "vectors vector_collection …" (+ the implicit "redis kv_collection")
redis-cli --user app-a --pass s3cr3t-a SS.PUT vectors a '{"$d":{"id":"a","payload":{"tag":"x"},"vector":[0.1,0.2,0.3]},"$type":"vector_collection/item","$v":1}'

aws --endpoint-url http://127.0.0.1:9010 s3 cp s3://vectors/a - | tee /tmp/a.canonical
# expected: byte-identical to the string written above (canonical, sorted keys)

# write back unchanged through WebDAV (Story 2 scenario 4a)
curl -su app-a:s3cr3t-a -T /tmp/a.canonical -H 'Content-Type: application/vnd.spacestorage.canonical+json; v=1' http://127.0.0.1:9020/vectors/a

curl -su app-a:s3cr3t-a -XPOST localhost:9200/vectors/_spacestorage/knn -H 'Content-Type: application/json' \
  -d '{"$d":{"k":1,"vector":[0.1,0.2,0.31]},"$type":"vector_collection/op-args","$v":1}' | jq '.["$d"][0].id'   # "a"

PGPASSWORD=s3cr3t-a psql -h 127.0.0.1 -U app-a -d tenant-a -c "SELECT * FROM spacestorage.knn('vectors', '[0.1,0.2,0.31]'::jsonb, 1);"
clickhouse-client --user app-a --password s3cr3t-a --database tenant-a -q "SHOW TABLES; DROP TABLE vectors;"
```

Expected: `SHOW TABLES` lists `vectors` with engine `SpaceStorage('vector_collection')`; after `DROP`, `redis-cli … SS.CONTAINERS` no longer lists it.

Automated matrix (every type × every protocol × create/list/read/write/alter/drop, plus canonical write-back): `cargo test -p spacestorage-conformance type_matrix`.

## 4. Quorum and timeout on every query (SC-004, SC-005)

```bash
# defaults: write TWO clamped to ONE on a single replica, read ONE
redis-cli --user app-a --pass s3cr3t-a SET k v
redis-cli --user app-a --pass s3cr3t-a SS.LAST
# expected: quorum ONE source=global clamped_from="TWO (replica_count=1)"; timeout 30s source=global

# explicit unsatisfiable → rejected (Clarification Q1)
redis-cli --user app-a --pass s3cr3t-a SS.WITH quorum THREE timeout 1000 SET k v     # -UNAVAILABLE requested=THREE available=1

# session options, Cassandra native consistency, PostgreSQL SET/hint, HTTP headers
PGPASSWORD=s3cr3t-a psql -h 127.0.0.1 -U app-a -d tenant-a -c "SET spacestorage.timeout = '2s';" -c "/*+ spacestorage: quorum=ONE */ SELECT 1;" -c "SELECT * FROM spacestorage.options;"
cqlsh 127.0.0.1 -u app-a -p s3cr3t-a -e "CONSISTENCY LOCAL_ONE; SELECT * FROM system.spacestorage_options;"
curl -si -u app-a:s3cr3t-a -H 'X-SpaceStorage-Timeout: 500ms' localhost:9200/ | grep -i x-spacestorage-applied   # Applied-Timeout: 500ms, Options-Source: …timeout=query

# timeout error within deadline + 1 s (uses the conformance slow-op hook in a debug build)
cargo test -p spacestorage-conformance timeout_deadline

# invalid values leave the session unchanged
redis-cli --user app-a --pass s3cr3t-a SS.OPTIONS SET quorum FIVE          # -ERR invalid option value: quorum=FIVE
redis-cli --user app-a --pass s3cr3t-a SS.OPTIONS GET                      # unchanged

# live reload of global defaults (FR-027)
sudo sed -i.bak 's/write_quorum TWO;/write_quorum ONE;/' /etc/spacestorage/node.conf && spacestorage reload
spacestorage config | grep query_defaults.write_quorum                     # ONE (configured)
```

## 5. Same answer from every protocol (SC-006)

```bash
PGPASSWORD=s3cr3t-a psql -h 127.0.0.1 -U app-a -d tenant-a -c "CREATE TABLE sales (id int PRIMARY KEY, region text, amount int);" \
  -c "INSERT INTO sales VALUES (1,'eu',10),(2,'eu',20),(3,'us',5);"
PGPASSWORD=s3cr3t-a psql -h 127.0.0.1 -U app-a -d tenant-a -tAc "SELECT region, sum(amount) FROM sales GROUP BY region ORDER BY region;"
clickhouse-client --user app-a --password s3cr3t-a --database tenant-a -q "SELECT region, sum(amount) FROM sales GROUP BY region ORDER BY region FORMAT TSV"
cqlsh 127.0.0.1 -u app-a -p s3cr3t-a -e "SELECT region, amount FROM tenant_a.sales WHERE region='eu' ALLOW FILTERING;"
curl -su app-a:s3cr3t-a localhost:9200/sales/_search -H 'Content-Type: application/json' \
  -d '{"size":0,"aggs":{"by":{"terms":{"field":"region"},"aggs":{"s":{"sum":{"field":"amount"}}}}}}' | jq '.aggregations.by.buckets[] | [.key, .s.value]'
spacestorage executions --limit 4 --output json | jq '.[].protocol'        # postgresql, clickhouse, cassandra, elasticsearch — all through the shared engine
```

Expected: `eu 30`, `us 5` from every protocol.

## 6. Namespace isolation (Story 1 scenarios 12–13)

```bash
AWS_ACCESS_KEY_ID=app-a AWS_SECRET_ACCESS_KEY=s3cr3t-a aws --endpoint-url http://127.0.0.1:9010 s3 mb s3://data
AWS_ACCESS_KEY_ID=app-b AWS_SECRET_ACCESS_KEY=s3cr3t-b aws --endpoint-url http://127.0.0.1:9010 s3 mb s3://data      # succeeds: different namespace
AWS_ACCESS_KEY_ID=app-b AWS_SECRET_ACCESS_KEY=s3cr3t-b aws --endpoint-url http://127.0.0.1:9010 s3 ls                # only tenant-b's bucket
redis-cli --user ops --pass 'very secret' PING                                     # -NOAUTH … principal has no namespace (FR-009a)
PGPASSWORD='very secret' psql -h 127.0.0.1 -U ops -d tenant-b -c "SELECT count(*) FROM spacestorage.containers;"   # admin may select any namespace
```

## 7. Wrong client, wrong port (SC-008)

```bash
time PGPASSWORD=x psql -h 127.0.0.1 -p 9042 -U app-a -d tenant-a -c 'select 1'      # refused < 1 s, protocol error
time redis-cli -p 5432 PING                                                        # refused < 1 s
time curl -s --max-time 2 http://127.0.0.1:6379/                                   # -ERR unknown protocol
spacestorage status | grep -E 'handshake_rejected|connections'                      # counters incremented; node healthy
```

Automated matrix (8 clients × 8 ports): `cargo test -p spacestorage-conformance mismatch_matrix`.

## 8. Drain with open protocol sessions (FR-040)

```bash
( PGPASSWORD=s3cr3t-a psql -h 127.0.0.1 -U app-a -d tenant-a -c "SELECT pg_sleep(5);" ) &
spacestorage stop --wait
# expected: new connections refused, the sleeping statement completes (or hits the drain timeout), node exits 0
```

## 9. Two-node cluster (SC-007) — deferred

Requires cluster membership and replication from features `04`/`06`. The conformance test `cross_node_read_after_write` exists and is `#[ignore]`d with that reason until then; the single-node coordinator behaviour (FR-032: node answers even when data would be remote) is exercised by `LocalEngine` trivially.

## 10. Automated equivalents

```bash
cargo test -p spacestorage-types            # canonical vectors, interim inventory
cargo test -p spacestorage-exec             # LocalEngine, option resolution, clamping, records
cargo test -p spacestorage-protocol-core    # option parsing, error rendering, signature detection
cargo test -p handler-postgresql -p handler-cassandra -p handler-redis -p handler-elasticsearch -p handler-clickhouse -p handler-s3 -p handler-webdav
cargo test -p spacestorage-conformance      # smoke per handler, type matrix, options, equivalence, mismatch matrix
```

Every success criterion maps to a test in `crates/conformance/tests/` as listed in [plan.md → Project Structure](plan.md#source-code-repository-root).
