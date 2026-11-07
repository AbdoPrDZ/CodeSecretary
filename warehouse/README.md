# CodeSecretary Warehouse

The cloud entry point for devices and the system of record. It handles device auth, ingest, the raw archive and agent config, and it owns **all** Postgres migrations. It never calculates time.

Plan: [.agents/plans/WAREHOUSE.md](../.agents/plans/WAREHOUSE.md) · Progress: [.agents/plans.progress/WAREHOUSE.progress.md](../.agents/plans.progress/WAREHOUSE.progress.md)

## Run locally

```sh
docker compose up -d --build            # Postgres + warehouse (migrations run on startup)
curl http://localhost:8080/healthz      # {"status":"ok"}
curl http://localhost:8080/readyz       # {"status":"ready"} once the DB is reachable
```

Or run natively against the compose Postgres:

```sh
docker compose up -d postgres
DATABASE_URL=postgres://secretary_warehouse:warehouse@localhost:5432/secretary \
DATABASE_MIGRATION_URL=postgres://secretary:secretary@localhost:5432/secretary \
cargo run -p secretary-warehouse
```

PowerShell: set `$env:DATABASE_URL = "..."` and `$env:DATABASE_MIGRATION_URL = "..."` first.

## Tests

```sh
docker compose up -d postgres
DATABASE_URL=postgres://secretary:secretary@localhost:5432/secretary cargo test -p secretary-warehouse
```

`tests/rls.rs` creates a fresh database per test, applies every migration, and checks the row-level security rules as an app login role, never as superuser.

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `DATABASE_URL` | required | App login. Must be a member of `secretary_app` and must not own the tables |
| `DATABASE_MIGRATION_URL` | unset | Owner connection; if set, migrations run at startup |
| `WAREHOUSE_BIND` | `0.0.0.0:8080` | Listen address |
| `DATABASE_MAX_CONNECTIONS` | `10` | Pool size |
| `LOG_FORMAT` | `text` | `text` or `json` |
| `RUST_LOG` | `info` | Log filter |

## Database security model

- Migrations run as the table owner. Services log in as roles in `secretary_app` (`NOLOGIN`, `NOBYPASSRLS`). Locally, `deploy/postgres/init/01-roles.sh` creates the `secretary_warehouse` login.
- Every transaction sets its tenant with `set_config('app.user_id', …, true)` through `Db::begin_for_user`. If it isn't set, every tenant table looks empty.
- Cross-RLS checks ("is this user a member of this workspace?") use `SECURITY DEFINER` helpers that only answer about the current user.
- `activity_events` is insert-only for services. It is partitioned monthly by the device timestamp `timestamp_start`, and `(event_id, timestamp_start)` de-duplicates re-sent events.
- `processing_jobs` is the only table without RLS. It's the engine's internal queue and holds only user IDs and days, not activity data.
