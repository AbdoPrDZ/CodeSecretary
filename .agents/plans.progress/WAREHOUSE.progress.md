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

### M1
- [ ] WH-4 — Keycloak token validation + device registration, Keycloak in Compose
  - Acceptance: agent signs in via Keycloak device flow and registers; bad tokens → 401; revoked device → 401
- [ ] WH-5 — Batch ingest (validate, dedupe, clock offset, insert)
  - Acceptance: re-sending a batch creates no duplicates
- [ ] WH-6 — Raw archive writer (S3-compatible local store TBD — MinIO images discontinued; S3/R2 prod)
  - Acceptance: every accepted batch archived; replay tool restores DB
- [ ] WH-7 — Job enqueue with debounce per (user, local day)
  - Acceptance: 100 batches in 1 min → 1 job per affected day
- [ ] WH-8 — Heartbeat endpoint
  - Acceptance: `devices.last_seen` updated
- [ ] WH-9 — Agent config endpoint with ETag
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

## Phase 3
- [ ] WH-30 — Workspace-level policies in agent config
- [ ] WH-31 — Self-hosting: configurable object store, migrations on startup, env docs

## Notes
- 2026-11-06: MinIO images are no longer published (pull denied). Removed from compose; pick an S3-compatible store in WH-6.
- 2026-11-06: CI `server` and `compose` jobs not yet run on GitHub (repo not pushed).
- 2026-11-07: Decision D14 — Keycloak replaces warehouse-issued device tokens. WH-4 rescoped; `device_authorizations` and `devices.token_*` (0002) are replaced by a new migration in WH-4.
