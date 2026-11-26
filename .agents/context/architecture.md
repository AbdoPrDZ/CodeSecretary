# Architecture

Status: implemented for every service (agent, warehouse, engine v1, overview); the raw archive (object storage), browser/shell collectors and AI are planned. Source: blueprint §6, §16; [GENERAL plan](../plans/GENERAL.md) §2–4.

## Shape

```
Developer machine                     Cloud
──────────────────                    ─────────────────────────────────────────
VS Code ext ─┐                        warehouse/  ingest + device API ─► Postgres (system of record)
Browser ext ─┼─► agent/ (Rust) ──HTTPS──►                           └─► object storage (raw archive)
Shell hooks ─┘   collect · redact ·                    │ enqueue (user, local day)
                 buffer · upload                       ▼
                                       engine/     correlation + time calculation (+ AI later)
                                                       │ sessions, evidence
                                                       ▼
                                       overview/   user API + React web dashboard
```

## Services and Responsibilities

| Service | Does | Never does |
|---|---|---|
| `shared/` | Event schema, validation, redaction, config/API types, project mapping rule semantics (Rust crate; TS types and JSON Schema generated) | I/O, DB, HTTP |
| `agent/` | Collect, redact, buffer (SQLite), upload; presence/focus, Git, FS collectors; hosts local API for extensions; VS Code extension (`agent/vscode`: editor events to the local API, sidebar panel that manages the agent through its CLI and `status.json`) | Calculate time; talk to DB |
| `warehouse/` | Device auth, ingest, dedupe, archive, agent config, enqueue jobs; **owns the DB schema** (Python, D15) | Calculate time; serve dashboard |
| `orm/` | `secretary-orm`: single-database Python ORM (from J-ERP) used by the Python services | Service logic |
| `engine/` | Correlate events → projects; build sessions; AI (Phase 2) | Serve HTTP to users |
| `overview/` | User sessions, workspaces, projects, corrections, reports, teams | **Compute time** — reads engine output only |
| Keycloak (infra) | Identity: sign-in, device grant, social login brokering, SSO (D14) | Authorization — workspaces/roles stay in Postgres |

## Ownership Boundaries (who writes what)

- Warehouse: `activity_events`, `devices`.
- Engine: non-user `event_assignments`, `work_sessions`, `session_evidence`, `coverage_gaps`, `ai_*`.
- Overview: users, workspaces, memberships, projects, mappings, exclusions, user overrides, `time_adjustments`, `submissions`.
- Anything that changes a calculation input **enqueues a job**; only Engine produces time.

## Key Principles

1. Raw events immutable; everything else derived and recomputable.
2. Deterministic engine; AI never sets durations.
3. Redaction in the agent before data leaves the device.
4. Multi-tenant from day one (workspaces + Postgres RLS), even though Phase 1 is single-user.
5. Private until submitted — teammates see only submitted totals.
6. Self-hostable via Docker Compose.

## Android App

Status: planned (Phase 2, D16). Source: blueprint §15, D16; [AGENT plan](../plans/AGENT.md) §3.5.

- Kotlin app in `agent/android/`, both a collector and a companion; iOS is not supported.
- **Same contract as the daemon:**
  - events from the SH-9 JSON Schema (SH-23)
  - the same device API
  - its own Keycloak client `secretary-android` (auth code + PKCE)
- **Collects only:** presence, from screen and lock events, and the foreground app as a package name, read in batches from `UsageStatsManager`. Never titles, notifications or content.

## Dashboard Runtime

Status: implemented (OV-1 scaffold, first slice). Source: `overview/`.

- **One image** (`overview/Dockerfile`): `secretary-overview` (FastAPI) serves the built React app (`STATIC_DIR`) on `:8090`. The same origin means no CORS, and the cookie stays first-party.
- **Database:** the API imports the warehouse's ORM models and RLS helpers, and never migrates; the warehouse does. It connects as an RLS-bound `secretary_app` member.
- **Web** (`overview/web`, a pnpm workspace package): React 19, Vite 7, TanStack Router + Query, Tailwind 4. It uses the TypeScript contract generated from `shared/` (`src/generated/contract.ts`).
- **Pages:** Time (default: per-project totals for a day, week or month; the day's session timeline with a session drawer for detail and corrections; the day's corrections with undo; recalculating state; time-zone prompt), Inbox (unassigned time by folder or app: assign or add a rule), Reports (range up to a year, project filter, CSV export), Activity (per-device day timeline of raw events; the table renders 300 rows at a time), Projects, Devices, Privacy (exclusions, pause windows, and "Your data": export, delete a day, a device or everything).
- **Queries:** default `staleTime` 30 s. The Time page polls every 5 s while days are recalculating.

## Engine Runtime

Status: implemented (EN-1, EN-2, engine v1). Source: `engine/src/`.

- **One binary**, `secretary-engine` (`engine/Dockerfile`, Debian slim). It is configured by env (`DATABASE_URL`, `ENGINE_WORKERS`, …; `config.rs`) and logs JSON with ids and counts only.
- **Database:** it connects as an RLS-bound `secretary_app` member; Compose reuses the warehouse's login. It never migrates, and uses runtime sqlx queries (no compile-time macros).
- **Tenancy:** the queue table has no RLS. All data access happens in a transaction with `app.user_id` set to the job's user.
- **Workers:** N async workers (default 2) plus a reaper (every 60 s). On SIGTERM they stop claiming and get 20 s to finish their current job.
- **Health:** `:8070` serves `/healthz` and `/readyz` (DB reachable, workers looped within 15 min). It is not published, and the image healthcheck uses bash `/dev/tcp`.
- **Pure core:** `model` (context, settings, day bounds via `chrono-tz`), `correlate` and `calculate`. Golden fixtures live in `engine/fixtures/` (`gen_fixtures.py` writes the inputs; `SECRETARY_BLESS=1` writes the expected outputs).
- **Rust version:** sqlx 0.9 needs Rust 1.94, so the engine crate sets its own `rust-version`.

## Agent Daemon Runtime

Status: implemented (AG-1 skeleton, AG-2/3/5/7; AG-4, AG-6 and AG-8 in progress). Source: `agent/daemon/src/`.

- One process per OS user. An exclusive OS file lock (`<data>/agent.lock`) blocks a second instance. The OS releases it on exit or crash, so stale locks are impossible.
- Directories come from OS conventions (`directories` crate, app name `secretary`). `SECRETARY_HOME` moves all of them to `<dir>/{config,data,logs}`. On Unix the directories are created with mode `0700`.
- `agent.toml` holds only bootstrap settings (server URL, log level). Settings the user manages in the dashboard come as remote `AgentConfig` (AG-7).
- Every long-running component is started through the `Supervisor` with a `CancellationToken`. On a stop signal (SIGINT/TERM/HUP, or Windows console close/logoff/shutdown) all tokens are cancelled. Components get a 4 s grace period, then they are aborted.
- Stable exit codes: 0 ok, 1 failure, 2 config, 3 already running.
- **Components (`run`):**
  - `presence`: a blocking thread ticking every 1 s. It samples idle time for presence intervals every 5 s, and the foreground window every tick for `window.focus` intervals (AG-9); both go into the pipeline.
  - `pipeline` (not a task): validate → exclusions → pause windows → redact → validate → buffer. Every collector goes through it; nothing else writes to the buffer.
  - `config`: `GET /v1/agent/config` every 5 min with the cached `ETag`.
  - `pause`: checks local time every 5 s and emits `agent.pause` / `agent.resume` at pause-window edges.
  - `git`: finds repositories under the project roots and polls each reflog every 5 s; new entries become `git.commit` / `git.checkout` / `git.merge` (no `git` process, no history). Roots: `agent.toml` `project_roots` plus the dashboard's, else the home folder.
  - `status`: writes `<data>/status.json` every 5 s, from a `StatusBoard` the other components update. The format is `secretary_shared::local`.
- **Status panel** (`agent/panel`, `secretary-panel`): a separate egui process that only reads `status.json`, so the daemon has no local server for it.
  - `uploader`: buffer → `POST /v1/events:batch`.
  - `heartbeat`: `POST /v1/agent/heartbeat`, which also measures the clock offset.
- **Shared state:** the uploader and heartbeat share a `Link` (sign-in state, access token) and a `ClockOffset`.
- **Files under `<data>`:**
  - `buffer.sqlite3`: a SQLite FIFO, WAL mode, bounded at 30 days or 500k rows.
  - `state.json`: sign-in settings and the device id, with no secrets.
  - `agent_config.json`: the last fetched `AgentConfig` and its `ETag`. It survives `logout` on purpose: events buffered while signed out are uploaded later, so they must still be filtered.
- **CLI-only commands:** `login`, `logout` and `status` never take the instance lock, so they work while the daemon runs.
- **Contract types** come from `secretary-shared` (`shared/`): the event envelope, the Phase 1 event catalog, validation, `AgentConfig` with the Appendix A defaults, and the device API bodies.
- OS integration is in `platform/`, one module per OS behind the same `Platform` API (`idle_time`, `focused_window`, `titles_available`). Collectors build on it (AG-8/AG-9). It returns raw titles, so callers must redact them before buffering and never log them. On macOS the app comes from `CGWindowListCopyWindowInfo`, which needs no run loop or permission. Titles come from the Accessibility API and need permission. On Linux only X11 is supported: Wayland works only for apps running under XWayland, and native Wayland support is AG-8.

## Stack

Rust (agent, engine, `shared/`; tokio, sqlx) · Python 3.13 (warehouse, overview API; FastAPI, uvicorn, `secretary-orm` on psycopg 3 — D15) · TypeScript (VS Code ext, browser ext, React + Vite web) · PostgreSQL · S3-compatible storage (local store TBD, WH-6) · Keycloak · SQLite in the agent · Docker Compose.

## Deployment Shape

Local dev and self-hosting: one Compose file (Postgres, Keycloak, warehouse, engine, overview now; S3-compatible store with WH-6). Managed hosting undecided (open decision OD2).

**Single VPS (`deploy/vps/`).** Status: implemented (2026-11-20), a test deployment, not the OD2 decision.
- **How it ships:** `deploy.py` builds the warehouse, overview and engine images locally (tag `beta` until production; each deploy replaces them) and streams them over SSH into `docker load`; there is no registry. It writes `compose.yml` and a `Caddyfile` to the folder named by `DIR` in the git-ignored `.ssh.env`.
- **Postgres:** the server's existing `it-pg-db`, reached on the external `it-db-net` network. Its databases and owner logins are the server owner's (`.env.docker` on the VPS). The deploy adds only `secretary_app` and `secretary_warehouse`, the RLS-bound runtime login.
- **TLS:** Caddy terminates TLS with a private CA certificate for the IP (`make_certs.py`). The warehouse is on `:8443`, Keycloak on `:8444`, and the issuer is `https://<ip>:8444/realms/secretary`. Clients must trust `deploy/vps/secrets/certs/ca.crt`.
- **Realm:** imported without the dev test user.
- **Services:** warehouse, overview (dashboard on `:8445`), engine (no published port) and Keycloak, all RLS-bound or on their own DB. Caddy compresses responses (`encode zstd gzip`) on `:8444` and `:8445`.
- **Keycloak admin:** the deploy's last step logs in with `KEYCLOAK_ADMIN_PASSWORD` from `secrets/.env.deploy` to point the dashboard client at the URL. Keycloak applies that password only when its database is first created, so if the admin password was changed in Keycloak the step fails (the deploy still exits 0, and the existing client keeps working).
