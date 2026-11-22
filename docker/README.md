# Three-node Docker Compose lab

Builds `spacestoraged` / `spacestorage` when the stack starts and runs a three-node cluster (`db-a` bootstrap, `db-b` / `db-c` join with one-time tokens).

```bash
docker compose up --build
```

`pull_policy: build` forces an image build on start even without `--build`.

Joiners wipe local `identity`/`raft` and remint a join token on every start so they contact the seed again (otherwise a restart leaves each node with a one-member view).

If bootstrap itself is half-initialized, wipe volumes:

```bash
docker compose down -v
docker compose up --build
```

## Host ports

| Node | PostgreSQL | Redis | admin-http |
|------|------------|-------|------------|
| db-a | 5432 | 6379 | 7701 |
| db-b | 5433 | 6380 | 7801 |
| db-c | 5434 | 6381 | 7901 |

## Smoke check

```bash
TOKEN=$(docker compose exec db-a cat /etc/spacestorage/secrets/admin.token)
docker compose exec -e SPACESTORAGE_TOKEN="$TOKEN" db-a \
  spacestorage membership --endpoint http://db-a:7701 --output json
```

Expect members `db-a`, `db-b`, `db-c`.

```bash
redis-cli -h 127.0.0.1 -p 6379 PING
psql "host=127.0.0.1 port=5432 dbname=postgres"
```
