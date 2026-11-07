# Database

Status: implemented for the schema (WH-2/WH-3, migrations 0001–0009 in `warehouse/migrations/`). Object storage and the agent SQLite buffer are still planned. Source: blueprint §9.3, §13; [WAREHOUSE plan](../plans/WAREHOUSE.md) §3.

## Systems

- **Postgres** — system of record for all cloud data.
- **Object storage** — append-only archive of raw upload batches (`raw/{user_id}/{yyyy}/{mm}/{dd}/{batch_id}.json.gz`); enables replay.
- **SQLite (agent only)** — local upload buffer; holds already-redacted events; not a source of truth.

## Data Categories

| Category | Examples | Rule |
|---|---|---|
| Raw | `activity_events` | Insert-only for the app role; deleted only via the privacy-delete procedure |
| Config / user data | projects, mappings, exclusions, `time_adjustments`, overrides | Written by Overview; changes enqueue recalculation |
| Derived | assignments, sessions, evidence, coverage gaps | Written only by Engine; replaced per (user, local day) in one transaction |
| Shared | `submissions` | The only per-user time data visible to teammates |
| Audit | `ai_request_log`, `audit_log` | Append-only |

## Rules

- All migrations live in `warehouse/migrations/` (sqlx, forward-only, numbered) — for every service.
- Every tenant-scoped table has `user_id` and/or `workspace_id`, RLS enabled, policy in the same migration.
- Services connect as a role **without** BYPASSRLS and set only `app.user_id` per transaction (`Db::begin_for_user`). Workspace access is derived from `memberships`, never from a separate setting. Never take the tenant from request bodies.
- `activity_events`: partitioned monthly by the device `timestamp_start`. The primary key `(event_id, timestamp_start)` makes ingest idempotent. It is indexed by `(user_id, server_adjusted_start)`. There's no default partition: `app_ensure_event_partitions()` keeps the range from 12 months back to 3 months ahead.
- Roles: migrations run as the owner. Services log in as members of `secretary_app` (NOLOGIN, NOBYPASSRLS). Tenant context comes from `app_current_user_id()`, which reads `app.user_id` and treats an empty value as unset. Cross-table checks use `SECURITY DEFINER` helpers (`app_is_member`, `app_is_admin`, …) that answer only about the current user.
- Pre-auth lookups (Keycloak `sub` → user with first-sight provisioning, `sid` → device, invite accept) must go through `SECURITY DEFINER` functions. Never relax a policy for them.
- Keycloak keeps its own database (separate DB in the same Postgres server) and is never read by services directly. `user_identities.provider` holds the Keycloak issuer.
- `processing_jobs` is the only table without RLS (system queue). `tests/rls.rs` fails if any other table lacks RLS or a policy.
- User corrections live in `assignment_overrides` and `time_adjustments`, never in derived tables.
- No FKs from derived tables to `activity_events` (a partitioned key would need `(event_id, timestamp_start)`). Privacy delete cleans derived rows explicitly.
- Reports are views/caches over sessions and submissions — never a source of truth.
- Timestamps stored in UTC; "local day" uses the user's configured time zone.
- No analytical warehouse (BigQuery/ClickHouse) for now — Postgres is sufficient (~1 GB/user/year).
