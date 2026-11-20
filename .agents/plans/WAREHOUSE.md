# Service Plan — Warehouse (`warehouse/`)

> **Blueprint:** [IDEA-V4.md](../blueprints/IDEA-V4.md) §9, §13, §14, Appendix C · **General plan:** [GENERAL.md](GENERAL.md)

## 1. Purpose

The **device-facing cloud entry point** and **system of record**:

- Device registration and request authentication (validates Keycloak access tokens, D14).
- Ingest API: authenticate, validate, deduplicate, store events immutably.
- Raw batch archive in object storage.
- Serve agent configuration.
- Enqueue engine jobs.
- **Owns the Postgres schema** for every service (ORM models + SQL, D15).

It does not calculate time and does not serve the dashboard.

## 2. Structure

```
warehouse/
├─ pyproject.toml            # secretary-warehouse (Python 3.13, FastAPI, secretary-orm)
├─ Dockerfile
├─ tests/                    # pytest: config, HTTP, RLS (fresh DB per test, app role)
└─ src/secretary_warehouse/
   ├─ __main__.py            # CLI: serve (default) | migrate | plan
   ├─ config.py              # env config
   ├─ telemetry.py           # text / JSON logging
   ├─ db.py                  # user_transaction / system_transaction (SET LOCAL app.user_id)
   ├─ http.py                # FastAPI app: /healthz, /readyz, /v1/...
   ├─ schema/                # ALL tables for every service
   │  ├─ models/             # ORM models (columns, indexes, FKs, checks)
   │  ├─ sql/pre.sql         # roles, partitioned activity_events shell
   │  └─ sql/post.sql        # RLS helpers + policies, grants, partitions, special indexes
   ├─ auth/                  # Keycloak JWT validation (JWKS cache), sub → user, sid → device
   ├─ device/                # register, revoke check
   ├─ ingest/                # batch handler, validation, dedupe, clock offset
   ├─ archive/               # S3-compatible writer
   ├─ config_api/            # GET /v1/agent/config
   ├─ heartbeat/
   ├─ jobs.py                # enqueue (user_id, local_day) with debounce key
   └─ ratelimit.py
```

## 3. Database (owned schema, D15)

Created in Phase 1 even if used later, so tenancy is never retrofitted. One ORM model module per area in `schema/models/`:

| Module | Tables |
|---|---|
| identity | `users`, `user_identities`, `workspaces`, `memberships`, `invitations` |
| devices | `devices` (one per Keycloak device session, `auth_session_id`) |
| events | `activity_events` (partitioned monthly by device `timestamp_start`, see rules) |
| projects | `projects`, `project_links`, `project_mappings`, `exclusions`, `rule_proposals` |
| derived | `event_assignments`, `work_sessions`, `session_evidence`, `coverage_gaps` |
| corrections | `assignment_overrides`, `time_adjustments` |
| jobs | `processing_jobs` |
| ai | `ai_classifications`, `ai_request_log` (Phase 2 use) |
| team | `submissions`, `audit_log` (Phase 3 use) |

**How it is applied:** `migrate()` (CLI `secretary-warehouse migrate`, or at startup when `DATABASE_MIGRATION_URL` is set) runs as the owner in one transaction, under an advisory lock: `pre.sql` → ORM sync (columns, indexes, then constraints: FKs from `ondelete`, selection CHECKs, `_sql_constraints`) → `post.sql`. Everything is idempotent. The ORM refuses type changes that can lose data and never drops columns unless `--allow-drop` is passed; `secretary-warehouse plan` previews the ORM part.

**Rules:**

- Every table has a `bigserial` `id` plus `created_at` / `updated_at` (ORM standard columns, `DEFAULT now()`). Natural keys are `UNIQUE` constraints. Agent-supplied event IDs stay UUIDv7 (`event_id`).
- Every tenant table: `user_id` and/or `workspace_id`, RLS enabled and policy in `post.sql`, added together with its model.
- `activity_events`: primary key `(id, timestamp_start)`; `UNIQUE (event_id, timestamp_start)` handles de-duplication. Postgres requires the partition key in every unique key, and `timestamp_start` (unlike `server_adjusted_start`) never changes for a given event. Index on `(user_id, server_adjusted_start)`.
- `activity_events` is **insert-only** for the app role (no UPDATE/DELETE grants); deletion only via the privacy-delete procedure.
- Partition maintenance: `app_ensure_event_partitions(months_back, months_ahead)`; every migration ensures 12 back + 3 ahead. No default partition.
- Sign-in: `app_resolve_identity(issuer, subject, email, name)` (SECURITY DEFINER) maps a Keycloak account to a user and provisions the user + personal workspace on first sight. The app role cannot insert into `users` directly.
- Index names in SQL end in `_ix` / `_ux`: the ORM drops undeclared indexes named `<table>_<column>_idx`.
- `sys_model` / `sys_field` (ORM bookkeeping for rename detection) are owner-only, no RLS, no grants.
- No RLS on `processing_jobs` (system queue, holds only `(user_id, local_day)`); every other table has RLS.
- Object storage: MinIO no longer publishes free images (late 2025). Choose an S3-compatible replacement for local dev/self-hosting in WH-6.

## 4. API

| Endpoint | Auth | Behaviour |
|---|---|---|
| `GET /v1/auth/config` | none | `{issuer, client_id}` for the agent (Keycloak realm + `secretary-agent` client) |
| `POST /v1/devices` | agent access token | Register the device for the token's Keycloak session (`sid`): name, OS, agent version. Idempotent per `sid` |
| `POST /v1/events:batch` | agent access token | Gzip JSON; ≤ 1,000 events / 1 MB; returns `{accepted:[ids], duplicates:[ids], rejected:[{id, reason}]}` |
| `GET /v1/agent/config` | agent access token | `AgentConfig` (exclusions, roots, pause windows, redaction version) + `ETag` |
| `POST /v1/agent/heartbeat` | agent access token | Updates `devices.last_seen`, `clock_offset_ms`, `agent_version`, queue depth |
| `GET /healthz`, `/readyz` | none | Liveness / readiness |

Device login happens entirely in Keycloak (device authorization grant). Device revocation is performed from Overview: it writes `devices.revoked_at` and deletes the device's Keycloak offline session. The warehouse rejects tokens whose `sid` belongs to a revoked device immediately, without waiting for the token to expire.

### 4.1 Batch Ingest Flow

1. Validate the access token → `(user_id, device_id)` (see §4.2); reject if the device is revoked or unregistered. All DB work for the batch runs in `user_transaction(user_id)`.
2. Decompress; enforce size limits.
3. Validate each event against the JSON Schema generated from `shared/` (SH-9) plus the same semantic checks; collect rejections.
4. Compute `server_adjusted_*` = device timestamps − reported clock offset.
5. Write raw batch to object storage: `raw/{user_id}/{yyyy}/{mm}/{dd}/{batch_id}.json.gz`.
6. `INSERT … ON CONFLICT (event_id) DO NOTHING` in one transaction; collect duplicates.
7. Compute affected local days (user time zone) → upsert `processing_jobs` with debounce (`run_after = now() + 2 min`).
8. Respond with accepted/duplicate/rejected IDs.

Raw archive write happens before DB insert; if DB insert fails, the archive allows replay.

### 4.2 Token Handling (Keycloak, D14)

- Access tokens are Keycloak JWTs (RS256). Check signature against the realm JWKS (cached, refetched on unknown `kid`), `iss`, `exp`/`nbf` (≤ 30 s leeway), `aud` contains `secretary-warehouse`, and `azp = secretary-agent`.
- `sub` → `users.id` via `app_resolve_identity()` (SECURITY DEFINER); first sight provisions the user and their personal workspace.
- `sid` (the Keycloak offline session created by the device login) → `devices.id`. One device per `sid`; re-login creates a new session and device row.
- `(sub, sid) → (user_id, device_id, revoked)` cached ≤ 60 s; revocation also evicts the cache entry.
- Never log tokens or full claims.
- `devices.auth_session_id` (unique) already exists (WH-1R); there is no `device_authorizations` table.

## 5. Tasks

### Phase 1

| ID | Task | Milestone | Acceptance |
|---|---|---|---|
| WH-1 | Service skeleton, HTTP server, config (env), health endpoints, logging | M0 | Boots in Docker Compose |
| WH-2 | Schema for every table with RLS policies | M0 | Migration clean and idempotent; RLS enabled on all tenant tables |
| WH-3 | DB access layer with per-transaction RLS context | M0 | Queries without context return nothing |
| WH-1R | Port WH-1–3 to Python: FastAPI + `secretary-orm`, ORM owns the schema (D15) | M0 | Same acceptance as WH-1–3; Compose healthy; RLS tests pass as the app role |
| WH-4 | Keycloak token validation + device registration (`/v1/auth/config`, `POST /v1/devices`), Keycloak in Compose with realm import | M1 | Agent signs in via Keycloak device flow and registers; forged/expired/wrong-audience tokens → 401; revoked device → 401 |
| WH-5 | Batch ingest (validate, dedupe, clock offset, insert) | M1 | Re-sending a batch creates no duplicates |
| WH-6 | Raw archive writer (S3-compatible local store TBD, S3/R2 in prod) | M1 | Every accepted batch archived; replay tool restores DB from archive |
| WH-7 | Job enqueue with debounce per (user, local day) | M1 | 100 batches in 1 min → 1 job per affected day |
| WH-8 | Heartbeat endpoint | M1 | `devices.last_seen` updated |
| WH-9 | Agent config endpoint with ETag | M1 | 304 when unchanged |
| WH-10 | Rate limiting per device | M1 | Excess requests → 429 with `Retry-After` |
| WH-11 | Monthly partition maintenance | M1 | Partitions exist 3 months ahead |
| WH-12 | Privacy delete procedure (user/day/device/all) incl. archive objects | M4 | Deleted data absent from DB and archive; derived data recalculated |
| WH-13 | Data export (all of a user's raw events as NDJSON) | M4 | Export matches DB contents |
| WH-14 | Isolation tests: two users, all endpoints and tables | M5 | Zero cross-access |
| WH-15 | Load test: 50 devices × 1 batch/min | M5 | p99 < 300 ms; no errors |
| WH-16 | Backups: nightly `pg_dump` / provider snapshots + restore drill | M5 | Restore verified |

### Phase 2

| ID | Task |
|---|---|
| WH-20 | Accept schema N and N-1 (terminal, browser, fs types) |
| WH-21 | Application-level encryption of sensitive metadata fields (envelope encryption, per-user data key, KMS / local key provider) |
| WH-22 | Retention job: compact raw events > 12 months; archive lifecycle rules |
| WH-23 | Token anomaly detection (new region/IP) → notify user |
| WH-24 | Accept the `secretary-android` client: Keycloak realm client (public, PKCE, offline access), `azp` allow-list, device `os = android` (D16) |

### Phase 3

| ID | Task |
|---|---|
| WH-30 | Workspace-level policies included in agent config |
| WH-31 | Self-hosting: configurable object store, schema migration on startup, documented env vars |

## 6. Testing

- Integration tests with `testcontainers` (Postgres + MinIO).
- Property test: random batch orders/duplicates → DB contains each event exactly once.
- Clock-offset tests (device ±10 min skew).
- RLS tests run as the app role, never as superuser.

## 7. Operational Targets

| Metric | Target |
|---|---|
| Ingest p99 latency | < 300 ms per batch |
| Availability (managed) | 99.5% (agent buffers through outages) |
| RPO / RTO | 24 h / 4 h (Phase 1); raw archive enables replay |

## 8. Risks

| Risk | Mitigation |
|---|---|
| RLS misconfiguration | Policy added with the model; test fails if any table lacks RLS; isolation test suite; app role without BYPASSRLS |
| Partition growth | Monthly partitions + retention job |
| Archive and DB diverge | Archive-first write; periodic reconciliation job (Phase 2) |
