# Database

Status: implemented for the schema (WH-1R: ORM models + SQL in `warehouse/src/secretary_warehouse/schema/`, D15). Object storage and the agent SQLite buffer are still planned. Source: blueprint §9.3, §13; [WAREHOUSE plan](../plans/WAREHOUSE.md) §3.

## Systems

- **Postgres** — system of record for all cloud data.
- **Object storage** — append-only archive of raw upload batches (`raw/{user_id}/{yyyy}/{mm}/{dd}/{batch_id}.json.gz`); enables replay.
- **SQLite (agent only)** — local upload buffer; holds already-redacted events; not a source of truth.

## Data Categories

| Category | Examples | Rule |
|---|---|---|
| Raw | `activity_events` | Insert-only for the app role; deleted only via the privacy-delete procedure |
| Dashboard sessions | `web_sessions` (token hash, CSRF token, Keycloak `sid`, expiry) | Overview; RLS by owner, looked up before a context exists via `app_web_session_user()` |
| Config / user data | projects, mappings, exclusions, `time_adjustments`, overrides | Written by Overview; changes enqueue recalculation |
| Derived | assignments, sessions, evidence, coverage gaps | Written only by Engine; replaced per (user, local day) in one transaction |
| Shared | `submissions` | The only per-user time data visible to teammates |
| Audit | `ai_request_log`, `audit_log` | Append-only |

## Rules

- The schema for every service lives in the warehouse: `secretary-orm` models define tables, columns, indexes, FKs (`ondelete`) and checks (selections, `_sql_constraints`); `sql/pre.sql` and `sql/post.sql` hold what the ORM cannot express (roles, partitioned events table, RLS helpers and policies, grants, expression/partial indexes). `migrate()` applies all of it idempotently in one owner transaction on every deployment; the ORM blocks changes that can lose data unless `--allow-drop` is given.
- Ids are `bigserial` on every table (plus ORM `created_at` / `updated_at` with `DEFAULT now()`); natural keys are `UNIQUE` constraints. Agent event IDs stay UUIDv7 in `event_id`.
- Every tenant-scoped table has `user_id` and/or `workspace_id`, RLS enabled, and its policy in `post.sql`, added in the same change as the model.
- Services connect as a role **without** BYPASSRLS and set only `app.user_id` per transaction (`user_transaction()` in the warehouse; the ORM shares that transaction). ORM calls outside it see nothing. Workspace access is derived from `memberships`, never from a separate setting. Never take the tenant from request bodies.
- `activity_events`: partitioned monthly by the device `timestamp_start` (the partitioned shell comes from `pre.sql`; the ORM adds the columns). `UNIQUE (event_id, timestamp_start)` makes ingest idempotent. It is indexed by `(user_id, server_adjusted_start)`. There's no default partition: `app_ensure_event_partitions()` keeps the range from 12 months back to 3 months ahead.
- Roles: `migrate()` runs as the owner. Services log in as members of `secretary_app` (NOLOGIN, NOBYPASSRLS). Tenant context comes from `app_current_user_id()`, which reads `app.user_id` and treats an empty value as unset. Cross-table checks use `SECURITY DEFINER` helpers (`app_is_member`, `app_is_admin`, …) that answer only about the current user.
- Pre-auth lookups (Keycloak `sub` → user with first-sight provisioning via `app_resolve_identity()`, `sid` → device, invite accept) must go through `SECURITY DEFINER` functions. Never relax a policy for them. The app role cannot insert users directly.
- Keycloak keeps its own database (`keycloak` DB and role on the same Postgres server; see `deploy/keycloak/README.md`) and is never read by services directly. `user_identities.provider` holds the Keycloak issuer.
- `processing_jobs`: at most one `queued` and one `running` job per (user, local day), both enforced by partial unique indexes. Producers upsert with `ON CONFLICT (user_id, local_day) WHERE status = 'queued' DO NOTHING`. `app_enqueue_uncalculated_days()` (SECURITY DEFINER) lets the engine find days that lack current results without reading other users' rows.
- Tables without RLS: `processing_jobs` (system queue) and the ORM's `sys_model` / `sys_field` (owner-only, no grants). `warehouse/tests/test_rls.py` fails if any other table lacks RLS or a policy.
- SQL index names end in `_ix` / `_ux`: the ORM drops undeclared indexes named `<table>_<column>_idx`.
- The ORM's `create` uses `INSERT … RETURNING`, which also needs SELECT access under RLS; append-only tables a role may write but not read (`audit_log` for members) need plain SQL inserts.
- User corrections live in `assignment_overrides` and `time_adjustments`, never in derived tables.
- No FKs from derived tables to `activity_events` (a partitioned key would need `(id, timestamp_start)`). Privacy delete cleans derived rows explicitly.
- Reports are views/caches over sessions and submissions — never a source of truth.
- Timestamps stored in UTC; "local day" uses the user's configured time zone.
- No analytical warehouse (BigQuery/ClickHouse) for now — Postgres is sufficient (~1 GB/user/year).
