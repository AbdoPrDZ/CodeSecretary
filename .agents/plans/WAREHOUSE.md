# Service Plan — Warehouse (`warehouse/`)

> **Blueprint:** [IDEA-V4.md](../blueprints/IDEA-V4.md) §9, §13, §14, Appendix C · **General plan:** [GENERAL.md](GENERAL.md)

## 1. Purpose

The **device-facing cloud entry point** and **system of record**:

- Device registration and request authentication (validates Keycloak access tokens, D14).
- Ingest API: authenticate, validate, deduplicate, store events immutably.
- Raw batch archive in object storage.
- Serve agent configuration.
- Enqueue engine jobs.
- **Owns all Postgres migrations** for every service.

It does not calculate time and does not serve the dashboard.

## 2. Structure

```
warehouse/
├─ Cargo.toml                # secretary-warehouse
├─ migrations/               # sqlx migrations — ALL tables, RLS policies
└─ src/
   ├─ main.rs                # Axum server, config, graceful shutdown
   ├─ db.rs                  # pool, per-request RLS context (SET LOCAL app.user_id)
   ├─ auth/                  # Keycloak JWT validation (JWKS cache), sub → user, sid → device
   ├─ device/                # register, revoke check
   ├─ ingest/                # batch handler, validation, dedupe, clock offset
   ├─ archive/               # S3-compatible writer (object_store crate)
   ├─ config_api/            # GET /v1/agent/config
   ├─ heartbeat/
   ├─ jobs.rs                # enqueue (user_id, local_day) with debounce key
   └─ ratelimit.rs
```

## 3. Database (owned migrations)

Created in Phase 1 even if used later, so tenancy is never retrofitted:

| Migration | Tables |
|---|---|
| 0001_identity | `users`, `workspaces`, `memberships`, `invitations` |
| 0002_devices | `devices`, `device_authorizations` |
| 0003_events | `activity_events` (partitioned monthly by device `timestamp_start`, see rules) |
| 0004_projects | `projects`, `project_links`, `project_mappings`, `exclusions`, `rule_proposals` |
| 0005_derived | `event_assignments`, `work_sessions`, `session_evidence`, `coverage_gaps` |
| 0006_corrections | `assignment_overrides`, `time_adjustments` |
| 0007_jobs | `processing_jobs` |
| 0008_ai | `ai_classifications`, `ai_request_log` (Phase 2 use) |
| 0009_team | `submissions`, `audit_log` (Phase 3 use) |

**Rules:**

- Every tenant table: `user_id` and/or `workspace_id`, RLS enabled, policy in the same migration.
- `activity_events`: primary key `(event_id, timestamp_start)` handles de-duplication. Postgres requires the partition key in every unique key, and `timestamp_start` (unlike `server_adjusted_start`) never changes for a given event. Index on `(user_id, server_adjusted_start)`.
- `activity_events` is **insert-only** for the app role (no UPDATE/DELETE grants); deletion only via the privacy-delete procedure.
- Partition maintenance: `app_ensure_event_partitions(months_back, months_ahead)`; the migration creates 12 back + 3 ahead. No default partition.
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

1. Validate the access token → `(user_id, device_id)` (see §4.2); reject if the device is revoked or unregistered.
2. Decompress; enforce size limits.
3. Validate each event with `shared::validate`; collect rejections.
4. Compute `server_adjusted_*` = device timestamps − reported clock offset.
5. Write raw batch to object storage: `raw/{user_id}/{yyyy}/{mm}/{dd}/{batch_id}.json.gz`.
6. `INSERT … ON CONFLICT (event_id) DO NOTHING` in one transaction; collect duplicates.
7. Compute affected local days (user time zone) → upsert `processing_jobs` with debounce (`run_after = now() + 2 min`).
8. Respond with accepted/duplicate/rejected IDs.

Raw archive write happens before DB insert; if DB insert fails, the archive allows replay.

### 4.2 Token Handling (Keycloak, D14)

- Access tokens are Keycloak JWTs (RS256). Check signature against the realm JWKS (cached, refetched on unknown `kid`), `iss`, `exp`/`nbf` (≤ 30 s leeway), `aud` contains `secretary-warehouse`, and `azp = secretary-agent`.
- `sub` → `users.id` via `user_identities (provider = issuer, subject = sub)` in a SECURITY DEFINER function; first sight provisions the user and their personal workspace.
- `sid` (the Keycloak offline session created by the device login) → `devices.id`. One device per `sid`; re-login creates a new session and device row.
- `(sub, sid) → (user_id, device_id, revoked)` cached ≤ 60 s; revocation also evicts the cache entry.
- Never log tokens or full claims.
- WH-4 adds a migration: `devices.auth_session_id` (unique) replaces `token_hash`/`token_prefix`; `device_authorizations` is dropped.

## 5. Tasks

### Phase 1

| ID | Task | Milestone | Acceptance |
|---|---|---|---|
| WH-1 | Crate skeleton, Axum server, config (env), health endpoints, tracing | M0 | Boots in Docker Compose |
| WH-2 | Migrations 0001–0009 with RLS policies | M0 | `sqlx migrate run` clean; RLS enabled on all tenant tables |
| WH-3 | DB access layer with per-transaction RLS context | M0 | Queries without context return nothing |
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

### Phase 3

| ID | Task |
|---|---|
| WH-30 | Workspace-level policies included in agent config |
| WH-31 | Self-hosting: configurable object store (MinIO), migrations on startup, documented env vars |

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
| RLS misconfiguration | Policy-in-same-migration rule; isolation test suite; app role without BYPASSRLS |
| Partition growth | Monthly partitions + retention job |
| Archive and DB diverge | Archive-first write; periodic reconciliation job (Phase 2) |
