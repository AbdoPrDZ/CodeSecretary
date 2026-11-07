# General Plan — Cloud Work Time Tracker

> **Source blueprint:** [IDEA-V4.md](../blueprints/IDEA-V4.md) — the blueprint is the "what and why"; these plans are the "how and in what order".
>
> **Service plans:** [SHARED.md](SHARED.md) · [AGENT.md](AGENT.md) · [WAREHOUSE.md](WAREHOUSE.md) · [ENGINE.md](ENGINE.md) · [OVERVIEW.md](OVERVIEW.md)

---

## 1. Objective

Deliver the **Personal MVP (Phase 1)** first: one user, several devices, accurate time per project — on a multi-tenant foundation so friends and teams (Phase 3) can be added without redesign.

**Phase 1 is done when** (blueprint §17):

- Calculated time is within **±15%** of the manual baseline log for one week.
- **≥ 80%** of active time is assigned to a project automatically.
- Tenant isolation tests pass.
- Redaction audit passes (zero secrets in stored data).

## 2. Services

| Service | Folder | Role | Language |
|---|---|---|---|
| **Shared** | `shared/` | Event schema, validation, redaction, config types — one source of truth | Rust crate |
| **Agent** | `agent/` | Collect, redact, buffer, upload (daemon + tray + VS Code / browser / shell integrations) | Rust, TypeScript, shell |
| **Warehouse** | `warehouse/` | Device auth, ingest API, Postgres system of record, raw archive, **owner of DB migrations** | Rust (Axum, sqlx) |
| **Engine** | `engine/` | Job queue worker: correlation, time calculation, AI | Rust (sqlx) |
| **Overview** | `overview/` | User auth, dashboard API, React web app: review, corrections, reports, teams | Rust (Axum) + React/TS |

## 3. Repository Layout

```
Secretary/
├─ Cargo.toml                 # Rust workspace: shared, agent, warehouse, engine, overview/api
├─ docker-compose.yml         # Local dev: postgres, keycloak, object storage, warehouse, engine, overview
├─ .github/workflows/         # CI
├─ shared/                    # crate: secretary-shared
├─ agent/
│  ├─ daemon/                 # crate: secretary-agent
│  ├─ vscode/                 # VS Code extension (TS)
│  ├─ browser/                # WebExtension (TS) — Phase 2
│  └─ shell/                  # Shell hooks — Phase 2
├─ warehouse/
│  ├─ src/                    # crate: secretary-warehouse (ingest + device API)
│  └─ migrations/             # ALL Postgres migrations (sqlx)
├─ engine/                    # crate: secretary-engine
├─ overview/
│  ├─ api/                    # crate: secretary-overview-api
│  └─ web/                    # React + Vite + TypeScript
└─ .agents/
   ├─ blueprints/
   └─ plans/
```

## 4. Cross-Service Contracts

These contracts are the boundaries between services. Change them only through `shared/` and a migration, never ad hoc.

| Contract | Producer → Consumer | Defined in |
|---|---|---|
| **Event schema** (`schema_version`, event types, metadata per type) | Agent → Warehouse → Engine | `shared/` (blueprint §9.2, Appendix B) |
| **Redaction rules** | Warehouse (config) → Agent (enforces) | `shared/` |
| **Agent config** (exclusions, project roots, pause windows) | Overview (edit) → Warehouse `GET /v1/agent/config` → Agent | `shared/` types, DB tables |
| **Device API** (auth config, register, batch, heartbeat) | Agent ↔ Warehouse | [WAREHOUSE.md §4](WAREHOUSE.md) |
| **Identity** (OIDC, device grant, realm `secretary`) | Keycloak → Agent, Overview, Warehouse | `deploy/keycloak/` realm export (D14) |
| **Database schema** | Warehouse migrations → all services | `warehouse/migrations/` |
| **Processing jobs** `(user_id, local_day, reason)` | Warehouse / Overview enqueue → Engine | `processing_jobs` table |
| **Derived data** (assignments, sessions, evidence) | Engine → Overview | DB tables (read-only for Overview) |
| **Corrections** (assignments, adjustments, mappings) | Overview → Engine (via jobs) | DB tables + job enqueue |

**Ownership rules:**

- Only **Warehouse** writes `activity_events` and `devices`.
- Only **Engine** writes `event_assignments` (non-user methods), `work_sessions`, `session_evidence`, `coverage_gaps`, `ai_*`.
- Only **Overview** writes users, workspaces, memberships, projects, mappings, adjustments, user overrides, submissions.
- Anyone who changes an input to calculation **enqueues a job**; nobody but Engine computes time.

## 5. Milestones (Phase 1)

| # | Milestone | Services | Result |
|---|---|---|---|
| **M0** | Foundations | all | Workspace, CI, Docker Compose, empty services boot, migrations run |
| **M1** | Event pipeline end-to-end | shared, agent, warehouse | Agent logs in via device flow and uploads heartbeats + presence events into Postgres |
| **M2** | Phase 1 collectors | agent | Presence/focus, Git (+ backfill), VS Code extension producing real events |
| **M3** | Engine v1 | engine | Rule-based correlation + deterministic sessions for (user, day) |
| **M4** | Dashboard v1 | overview | Login, projects/mappings, Today/Week timeline, evidence, corrections, inbox, CSV, devices |
| **M5** | Hardening & validation | all | Redaction audit, isolation tests, performance targets, 1-week baseline comparison |

### 5.1 Dependency Graph

```
M0 ──► M1 ──► M2 ──┐
         │         ├──► M5
         └──► M3 ──┤
              │    │
              └──► M4
```

- M3 can start once M1 stores events (use fixture events before M2 is complete).
- M4 can start once M0 migrations exist (build against fixture sessions before M3 is complete).

### 5.2 Suggested Sequence (solo developer)

| Weeks | Focus |
|---|---|
| 1 | Phase 0 validation (watcher prototype on 3 OSes, manual baseline log starts) + M0 |
| 2–3 | M1 |
| 3–5 | M2 (presence/focus → Git → VS Code) |
| 5–7 | M3 |
| 6–9 | M4 |
| 9–10 | M5 (baseline comparison needs one full week of real data) |

## 6. Later Phases (summary)

| Phase | Shared | Agent | Warehouse | Engine | Overview |
|---|---|---|---|---|---|
| **2 — Collectors & AI** | Event types for terminal/browser/fs | Terminal hooks, browser ext, FS tracker | Native-messaging-independent; batch limits review | AI classification, rule suggestions, summaries, coverage-gap views | AI settings, AI log view, health views; field encryption |
| **3 — Friends & Teams** | Team config types | Team project links | Invite-only signup support, rate limits | Shared rules, resubmission notices | Workspaces, roles, invites, submissions, team reports, audit log, self-host package |
| **4 — Billing & Enterprise** | — | — | — | Rounding, billable/waiting policy | Rates, budgets, Excel, integrations, SSO |

Detailed tasks for each phase are in the service plans.

## 7. Engineering Conventions

### 7.1 Rust

- Single Cargo workspace; shared dependency versions in the root `Cargo.toml`.
- `tokio` runtime, `axum` for HTTP, `sqlx` (Postgres, compile-time checked queries), `serde`, `tracing`, `thiserror`/`anyhow`.
- `cargo fmt`, `cargo clippy -D warnings` required in CI.

### 7.2 TypeScript

- `pnpm` workspaces for `agent/vscode`, `agent/browser`, `overview/web`.
- Strict TypeScript, ESLint, Prettier.
- Shared TS types generated from `shared/` (e.g., `ts-rs` or JSON Schema → TS) — never hand-copied.

### 7.3 Database

- All migrations in `warehouse/migrations/`, forward-only, numbered.
- Every tenant-scoped table has `user_id` and/or `workspace_id` and an **RLS policy** in the same migration that creates it.
- Services connect with a role that is subject to RLS; set `app.user_id` / `app.workspace_id` per transaction.

### 7.4 Testing

| Level | Tooling | Required for |
|---|---|---|
| Unit | `cargo test`, Vitest | All logic, especially redaction and calculation |
| Integration | `testcontainers` Postgres + MinIO | Warehouse, Engine, Overview API |
| Golden tests | Fixture event days → expected sessions (JSON snapshots) | Engine — any change to output must update goldens consciously |
| Isolation tests | Two users / two workspaces; assert zero cross-access | Every table and every API endpoint |
| Redaction audit | Corpus of secret-bearing samples → assert none survive | Shared + Agent |
| E2E | Agent → Warehouse → Engine → Overview on Docker Compose | M5 and each release |

### 7.5 CI

- On every PR: fmt, clippy, unit, integration, isolation, redaction, TS lint/test, web build.
- On `main`: build container images (warehouse, engine, overview), build agent binaries for Windows/macOS/Linux.

### 7.6 Versioning and Releases

- `schema_version` bumps on any event schema change; Warehouse accepts current and previous version.
- `algorithm_version` bumps on any change that alters Engine output.
- Agent releases are signed; auto-update checks a release manifest.

### 7.7 Observability

- `tracing` + OpenTelemetry for all services.
- **Never log event metadata** (titles, URLs, paths, commands). Log IDs and counts only.

## 8. Environments

| Environment | Composition |
|---|---|
| **Local dev** | `docker compose up`: Postgres 16, MinIO, warehouse, engine, overview (api + web dev server); agent runs natively pointing at `http://localhost` |
| **Staging** | Same images; managed Postgres; S3-compatible bucket |
| **Production (managed)** | Chosen in Phase 0 (open question); TLS, backups, monitoring |
| **Self-hosted** (Phase 3) | Published `docker-compose.yml` + docs |

## 9. Definition of Done (any task)

- Code merged with tests; CI green.
- Contracts updated in `shared/` and migrations if touched.
- No raw metadata in logs.
- RLS policy present for new tenant tables; isolation test added.
- Relevant plan checkbox ticked and blueprint updated if a decision changed.

## 10. Open Decisions to Resolve Early

| # | Decision | Needed by | Default if not decided |
|---|---|---|---|
| OD1 | Build own presence watcher vs. reuse ActivityWatch code | M2 | Build own (Rust), borrow approach |
| OD2 | Managed hosting provider/region | M5 | Defer; run on Docker Compose on a single VPS |
| OD3 | Tray implementation (Tauri vs. `tray-icon` crate) | M2 | `tray-icon` crate (lighter) |
| OD4 | Rust → TS type generation tool | M1 | `ts-rs` |
| OD5 | Postgres job queue: custom SKIP LOCKED vs. pgmq | M3 | Custom SKIP LOCKED table |
| OD6 | Non-coding time (email, chat, meetings) handling | Phase 2 | Ignored (unassigned, not counted) |

## 11. Risks to Watch During Phase 1

| Risk | Early signal | Response |
|---|---|---|
| Presence detection unreliable on one OS | Phase 0 prototype gaps | Ship that OS as best-effort; document |
| Accuracy far from baseline | M5 comparison > ±15% | Tune Appendix A defaults; add evidence views to see why |
| Scope creep into Phase 2/3 features | Tasks outside M0–M5 | Park in the later-phase sections of service plans |
| Solo-builder ops burden | Time spent on infra | Keep single VPS + Compose until users exist |
