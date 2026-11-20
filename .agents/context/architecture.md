# Architecture

Status: planned. Source: blueprint §6, §16; [GENERAL plan](../plans/GENERAL.md) §2–4.

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
| `shared/` | Event schema, validation, redaction, config/API types (Rust crate; TS types and JSON Schema generated) | I/O, DB, HTTP |
| `agent/` | Collect, redact, buffer (SQLite), upload; presence/focus, Git, FS collectors; hosts local API for extensions | Calculate time; talk to DB |
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

## Agent Daemon Runtime

Status: implemented (AG-1 skeleton, AG-2/3/5; AG-4 and AG-8 in progress). Source: `agent/daemon/src/`.

- One process per OS user. An exclusive OS file lock (`<data>/agent.lock`) blocks a second instance. The OS releases it on exit or crash, so stale locks are impossible.
- Directories come from OS conventions (`directories` crate, app name `secretary`). `SECRETARY_HOME` moves all of them to `<dir>/{config,data,logs}`. On Unix the directories are created with mode `0700`.
- `agent.toml` holds only bootstrap settings (server URL, log level). Settings the user manages in the dashboard come as remote `AgentConfig` (AG-7).
- Every long-running component is started through the `Supervisor` with a `CancellationToken`. On a stop signal (SIGINT/TERM/HUP, or Windows console close/logoff/shutdown) all tokens are cancelled. Components get a 4 s grace period, then they are aborted.
- Stable exit codes: 0 ok, 1 failure, 2 config, 3 already running.
- **Components (`run`):**
  - `presence`: a blocking thread polling idle time every 5 s; it emits intervals into the buffer.
  - `uploader`: buffer → `POST /v1/events:batch`.
  - `heartbeat`: `POST /v1/agent/heartbeat`, which also measures the clock offset.
- **Shared state:** the uploader and heartbeat share a `Link` (sign-in state, access token) and a `ClockOffset`.
- **Files under `<data>`:**
  - `buffer.sqlite3`: a SQLite FIFO, WAL mode, bounded at 30 days or 500k rows.
  - `state.json`: sign-in settings and the device id, with no secrets.
- **CLI-only commands:** `login`, `logout` and `status` never take the instance lock, so they work while the daemon runs.
- **Contract types** come from `secretary-shared` (`shared/`): the event envelope, the Phase 1 event catalog, and the device API bodies.
- OS integration is in `platform/`, one module per OS behind the same `Platform` API (`idle_time`, `focused_window`, `titles_available`). Collectors build on it (AG-8/AG-9). It returns raw titles, so callers must redact them before buffering and never log them. On macOS the app comes from `CGWindowListCopyWindowInfo`, which needs no run loop or permission. Titles come from the Accessibility API and need permission. On Linux only X11 is supported: Wayland works only for apps running under XWayland, and native Wayland support is AG-8.

## Stack

Rust (agent, engine, `shared/`; tokio, sqlx) · Python 3.13 (warehouse, overview API; FastAPI, uvicorn, `secretary-orm` on psycopg 3 — D15) · TypeScript (VS Code ext, browser ext, React + Vite web) · PostgreSQL · S3-compatible storage (local store TBD, WH-6) · Keycloak · SQLite in the agent · Docker Compose.

## Deployment Shape

Local dev and self-hosting: one Compose file (Postgres, Keycloak, warehouse now; S3-compatible store (WH-6), engine and overview later). Managed hosting undecided (open decision OD2).
