# Progress — Warehouse (`warehouse/`)

> **Plan:** [WAREHOUSE.md](../plans/WAREHOUSE.md) · **Overall:** [GENERAL.progress.md](GENERAL.progress.md)
>
> Tick only when acceptance criteria are met. Format: `- [x] ID — task · YYYY-MM-DD · note`. In progress: `· 🚧`. Blocked: `· ⛔ reason`.

## Phase 1

### M0
- [x] WH-1 — Crate skeleton, Axum server, env config, health endpoints, tracing · 2026-11-06 · boots healthy in Docker Compose; /healthz + /readyz; JSON logs
  - Acceptance: boots in Docker Compose
- [x] WH-2 — Migrations 0001–0009 with RLS policies · 2026-11-06 · RLS + policy on every table except processing_jobs (by design); events partitioned by device timestamp_start (deviation, see plan §3)
  - [x] 0001_identity
  - [x] 0002_devices
  - [x] 0003_events
  - [x] 0004_projects
  - [x] 0005_derived
  - [x] 0006_corrections
  - [x] 0007_jobs
  - [x] 0008_ai
  - [x] 0009_team
  - Acceptance: `sqlx migrate run` clean; RLS enabled on all tenant tables
- [x] WH-3 — DB access layer with per-transaction RLS context · 2026-11-06 · Db::begin_for_user / begin_unscoped; 14 RLS tests as app role (no-context = empty, no leak across pooled tx)
  - Acceptance: queries without context return nothing
- [x] WH-1R — Port warehouse to Python (FastAPI + `secretary-orm`, ORM owns the schema; D15) · 2026-11-07 · Rust crate + sqlx migrations removed (history: f6d22db); 22 tenant tables with RLS, 42 FKs, 30 checks, 16 partitions; migrate() idempotent (restart = 0 changes); 35 warehouse tests (RLS as app role) + 69 orm tests; Compose healthy; CI job switched to Python
  - Acceptance: same behaviour as WH-1–3: boots in Compose, /healthz + /readyz, schema created by ORM + post-sync SQL, RLS tests pass as the app role

### M1
- [x] WH-4 — Keycloak token validation + device registration, Keycloak in Compose · 2026-11-14 · Keycloak 26.4.2 in Compose + realm import (smoke test 14/14, in CI); `auth.py`: RS256 via JWKS, iss/aud/azp/typ/exp, sub → user via `app_resolve_identity`, sid → device, 60 s cache; `GET /v1/auth/config`, `POST /v1/devices`; tests: 11 bad-token cases → 401, revoked → 401; E2E: the real agent signed in through Keycloak and registered in Compose
  - Acceptance: agent signs in via Keycloak device flow and registers; bad tokens → 401; revoked device → 401
- [x] WH-5 — Batch ingest (validate, dedupe, clock offset, insert) · 2026-11-07 · `POST /v1/events:batch` (gzip or JSON, ≤1 MB sent / 16 MB inflated / 1,000 events), per-event rejections, `ON CONFLICT (event_id, timestamp_start) DO NOTHING`; test: resent batch → all duplicates, 5 rows; archive (WH-6) and job enqueue (WH-7) not wired yet
  - Acceptance: re-sending a batch creates no duplicates
- [ ] WH-6 — Raw archive writer (S3-compatible local store TBD — MinIO images discontinued; S3/R2 prod)
  - Acceptance: every accepted batch archived; replay tool restores DB
- [ ] WH-7 — Job enqueue with debounce per (user, local day)
  - Acceptance: 100 batches in 1 min → 1 job per affected day
- [x] WH-8 — Heartbeat endpoint · 2026-11-07 · `POST /v1/agent/heartbeat` updates last_seen_at, clock_offset_ms, agent_version, queue_depth; returns `server_time` (agent derives clock offset)
  - Acceptance: `devices.last_seen` updated
- [x] WH-9 — Agent config endpoint with ETag · 2026-11-20 · `GET /v1/agent/config` (`agent_config.py`): body = `secretary_shared::config::AgentConfig` built from the user's `exclusions` under RLS (path/app/domain → exclusions, time_window → pause_windows), Appendix A agent timings; ETag = hash of the body, `If-None-Match` (strong or weak) → 304; tests: defaults + 304, exclusion change → new ETag + body, other user sees nothing, 401/403 without device. Project roots always `[]` (no storage yet)
  - Acceptance: 304 when unchanged
- [ ] WH-10 — Rate limiting per device
  - Acceptance: excess → 429 with `Retry-After`
- [ ] WH-11 — Monthly partition maintenance
  - Acceptance: partitions exist 3 months ahead

### M4
- [ ] WH-12 — Privacy delete procedure (user/day/device/all) incl. archive
  - Acceptance: data absent from DB and archive; derived data recalculated
- [ ] WH-13 — Data export (raw events as NDJSON)
  - Acceptance: export matches DB contents

### M5
- [ ] WH-14 — Isolation tests: two users, all endpoints and tables
  - Acceptance: zero cross-access
- [ ] WH-15 — Load test: 50 devices × 1 batch/min
  - Acceptance: p99 < 300 ms; no errors
- [ ] WH-16 — Backups + restore drill
  - Acceptance: restore verified

## Phase 2
- [ ] WH-20 — Accept schema N and N-1 (terminal, browser, fs types)
- [ ] WH-21 — Application-level encryption of sensitive metadata
- [ ] WH-22 — Retention job + archive lifecycle rules
- [ ] WH-23 — Token anomaly detection → notify user
- [ ] WH-24 — Accept the `secretary-android` client (realm client, `azp` allow-list) (D16)

## Phase 3
- [ ] WH-30 — Workspace-level policies in agent config
- [ ] WH-31 — Self-hosting: configurable object store, migrations on startup, env docs

## Notes
- 2026-11-06: MinIO images are no longer published (pull denied). Removed from compose; pick an S3-compatible store in WH-6.
- 2026-11-06: CI `server` and `compose` jobs not yet run on GitHub (repo not pushed).
- 2026-11-07: Decision D14 — Keycloak replaces warehouse-issued device tokens. WH-4 rescoped.
- 2026-11-07: Decision D15 — warehouse rewritten in Python on `secretary-orm`; the ORM owns the schema. WH-1–3 stay ticked as history; WH-1R carries the current implementation. Ids are now bigserial (event_id stays UUIDv7). The D14 schema change is already in: `devices.auth_session_id`, no `device_authorizations`. Added early: `app_resolve_identity()` (sign-in provisioning; WH-4 still owns token validation).
- 2026-11-07: ORM changes needed for this: UUID/time/smallint fields, real FKs (`ondelete`), selection CHECKs, `_sql_constraints`, NULL for `default=None`, `DEFAULT now()` on auto timestamps.
- 2026-11-07: Keycloak findings: users imported from a realm file get no default roles (no `offline_access` → `not_allowed`), so the dev user lists `default-roles-secretary`; the device grant always shows a consent page; login cookies are `Secure` even over http://localhost.
- 2026-11-07: Local dev database reset (`public` schema dropped; it held only the old sqlx bookkeeping, no data) so the new schema could be created.
- 2026-11-07: Device API error contract: `{"error": code}`; 401 `invalid_token` / `device_revoked` (with `WWW-Authenticate`), 403 `device_not_registered` (agent re-registers), 413 `batch_too_large`, 400 `invalid_body` (FastAPI's 422 replaced: it echoes input), 503 `auth_unavailable` when JWKS can't be fetched.
- 2026-11-07: Event validation lives in `ingest.py` until SH-4/SH-9: v7 event ids, schema_version 1, catalog types, `name@version` source, tz-qualified timestamps, end ≥ start, ≤ 5 min future (adjusted), ≤ 360 days old (partition range), metadata object ≤ 8 KB. Per-type metadata rules come with SH-4.
- 2026-11-07: Revocation takes effect within the 60 s lookup cache (plan §4.2); overview's revoke (OV-4) also ends the Keycloak session so refresh fails at once.
