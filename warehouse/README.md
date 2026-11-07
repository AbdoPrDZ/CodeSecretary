# CodeSecretary Warehouse

The cloud entry point for devices and the system of record. It handles device auth, ingest, the raw archive and agent config, and it owns the **database schema for every service**. It never calculates time.

Python 3.13 · FastAPI + uvicorn · [`secretary-orm`](../orm/README.md) on psycopg 3 (decision D15).

Plan: [.agents/plans/WAREHOUSE.md](../.agents/plans/WAREHOUSE.md) · Progress: [.agents/plans.progress/WAREHOUSE.progress.md](../.agents/plans.progress/WAREHOUSE.progress.md)

## Run locally

```sh
docker compose up -d --build            # Postgres + warehouse (schema migrates on startup)
curl http://localhost:8080/healthz      # {"status":"ok"}
curl http://localhost:8080/readyz       # {"status":"ready"} once the DB is reachable
```

Or run natively against the compose Postgres:

```sh
pip install -e orm -e "warehouse[test]"
docker compose up -d postgres
DATABASE_URL=postgresql://secretary_warehouse:warehouse@localhost:5432/secretary \
DATABASE_MIGRATION_URL=postgresql://secretary:secretary@localhost:5432/secretary \
secretary-warehouse                      # or: python -m secretary_warehouse
```

PowerShell: set `$env:DATABASE_URL = "..."` and `$env:DATABASE_MIGRATION_URL = "..."` first.

Commands: `serve` (default), `migrate` (sync the schema and exit), `plan` (print the ORM changes a migration would make). `--allow-drop` lets `migrate`/`plan` drop columns that are no longer declared. Exit codes: 0 ok, 1 failure, 2 configuration error.

## Tests

```sh
docker compose up -d postgres
DATABASE_URL=postgresql://secretary:secretary@localhost:5432/secretary python -m pytest warehouse/tests
```

`tests/test_rls.py` creates a fresh database per test, runs `migrate()`, and checks the row-level security rules as an app login role, never as superuser. Without `DATABASE_URL` only the config and no-database HTTP tests run.

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `DATABASE_URL` | required | App login. Must be a member of `secretary_app` and must not own the tables |
| `DATABASE_MIGRATION_URL` | unset | Owner connection; if set, the schema migrates at startup |
| `WAREHOUSE_BIND` | `0.0.0.0:8080` | Listen address |
| `DATABASE_MAX_CONNECTIONS` | `10` | Pool size |
| `LOG_FORMAT` | `text` | `text` or `json` |
| `LOG_LEVEL` | `info` | `debug`, `info`, `warning`, `error` |

## Schema

Everything lives in [src/secretary_warehouse/schema/](src/secretary_warehouse/schema/):

- `models/`: ORM models for every table. They declare columns, indexes, foreign keys (`Many2oneField(..., ondelete=...)`), selection checks and `_sql_constraints`. Other Python services import these models; they never define tables themselves.
- `sql/pre.sql`: roles and the partitioned `activity_events` table. It runs before the ORM sync.
- `sql/post.sql`: RLS helper functions and policies, grants, sign-in provisioning (`app_resolve_identity`), event partitions and expression/partial indexes. It runs after the sync.

`migrate()` runs all three steps as the owner, in one transaction, under an advisory lock. Each step is idempotent, so it runs on every start. The ORM refuses changes that could lose data, such as narrowing a type or dropping a column, unless you allow them explicitly.

**Adding a table:** add a model, then add its `ENABLE ROW LEVEL SECURITY`, its policy and its grants to `post.sql`, then add an isolation test. `test_every_tenant_table_has_rls_and_a_policy` fails if you forget the policy.

## Database security model

- Migrations run as the table owner. Services log in as roles in `secretary_app` (`NOLOGIN`, `NOBYPASSRLS`). Locally, `deploy/postgres/init/01-roles.sh` creates the `secretary_warehouse` login.
- Every transaction sets its tenant with `set_config('app.user_id', …, true)` through `user_transaction(db, user_id)`. ORM calls inside the block share that transaction. Outside it, every tenant table looks empty.
- Cross-RLS checks ("is this user a member of this workspace?") use `SECURITY DEFINER` helpers that only answer about the current user. New users are created only by `app_resolve_identity()`; the app role cannot insert into `users`.
- `activity_events` is insert-only for services. It is partitioned monthly by the device timestamp `timestamp_start`, and `UNIQUE (event_id, timestamp_start)` de-duplicates re-sent events.
- Tables without RLS:
  - `processing_jobs`: the engine's queue. It holds only user IDs and days, not activity data.
  - `sys_model` and `sys_field`: the ORM's own bookkeeping. Only the owner can access them; the app role has no grants.
