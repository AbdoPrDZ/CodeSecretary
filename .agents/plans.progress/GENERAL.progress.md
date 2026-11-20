# Progress — General

> **Plan:** [GENERAL.md](../plans/GENERAL.md) · **Service progress:** [SHARED](SHARED.progress.md) · [AGENT](AGENT.progress.md) · [WAREHOUSE](WAREHOUSE.progress.md) · [ENGINE](ENGINE.progress.md) · [OVERVIEW](OVERVIEW.progress.md)

## How to Update

- Tick a box (`- [x]`) only when the task meets its **acceptance criteria** in the plan and the **Definition of Done** (GENERAL.md §9).
- Add the completion date and a short note: `- [x] SH-2 — IDs + UUIDv7 · 2026-11-10 · notes`.
- In progress: keep `- [ ]` and add `· 🚧 in progress`. Blocked: add `· ⛔ blocked: <reason>`.
- A milestone is ticked here only when **all** its tasks in every service file are done.
- Update the **Status** table below whenever a milestone changes.

## Status

| Phase / Milestone | Status | Updated |
|---|---|---|
| Phase 0 — Validation | In progress | presence/focus probe (AG-0.1) verified on Windows |
| M0 — Foundations | In progress | CI, agent skeleton (AG-1, awaiting CI on macOS/Linux), Python warehouse (WH-1R) + `secretary-orm` done, `shared` crate in (SH-1 awaiting CI); engine/overview M0 pending |
| M1 — Event pipeline end-to-end | In progress | demo works on Windows: agent signs in via Keycloak, uploads heartbeats + presence events (AG-2/3/5/7, WH-4/5/8/9, SH-2/3/4/8 done; AG-4 1 h offline run, AG-6 redact step, WH-6/7/10/11, SH-5/6/7/9, OV M1 left) |
| M2 — Phase 1 collectors | Not started | — |
| M3 — Engine v1 | Not started | — |
| M4 — Dashboard v1 | Not started | — |
| M5 — Hardening & validation | Not started | — |
| Phase 2 — Collectors & AI | Not started | — |
| Phase 3 — Friends & Teams | Not started | — |
| Phase 4 — Billing & Enterprise | Not started | — |

## Phase 0 — Validation

- [ ] AG-0.1 — Presence + focus prototype on Windows, macOS, Linux
- [ ] AG-0.2 — Evaluate ActivityWatch watcher reuse (resolves OD1)
- [ ] Choose hosting for the managed version (resolves OD2, or defer)
- [ ] Start 2-week manual time log (accuracy baseline)
- [ ] Finish 2-week manual time log

**Exit criteria**
- [ ] Watcher prototype works on all three OSes
- [ ] Hosting decision recorded (or deferred)
- [ ] Baseline log collected

## Phase 1 — Milestones

### M0 — Foundations
- [ ] Root Cargo workspace (`shared`, `agent/daemon`, `engine`) — shared + agent in; engine pending. Python packages: `orm`, `warehouse` (overview/api later, D15)
- [ ] pnpm workspace (`agent/vscode`, `overview/web`)
- [ ] `docker-compose.yml` (Postgres 16, Keycloak, object storage, warehouse, engine, overview) — Postgres, Keycloak, warehouse in
- [ ] CI: fmt, clippy, tests, TS lint/test, web build
- [ ] SHARED M0 tasks done
- [ ] AGENT M0 tasks done
- [x] WAREHOUSE M0 tasks done
- [ ] ENGINE M0 tasks done
- [ ] OVERVIEW M0 tasks done

### M1 — Event pipeline end-to-end
- [ ] SHARED M1 tasks done
- [ ] AGENT M1 tasks done
- [ ] WAREHOUSE M1 tasks done
- [ ] OVERVIEW M1 tasks done (device approval)
- [x] Demo: agent signs in via device flow and uploads heartbeats + presence events into Postgres · 2026-11-14 · Windows + Docker Compose: `login` → Keycloak approval → device registered → presence intervals and heartbeats stored; restart keeps the sign-in; revocation signs the agent out

### M2 — Phase 1 collectors
- [ ] AGENT M2 tasks done
- [ ] OVERVIEW M2 tasks done (tray endpoint)
- [ ] Demo: presence, focus, Git, and VS Code events flowing for a full workday

### M3 — Engine v1
- [ ] ENGINE M3 tasks done
- [ ] Demo: sessions produced for real (user, day) data; golden tests green

### M4 — Dashboard v1
- [ ] WAREHOUSE M4 tasks done (privacy delete, export)
- [ ] OVERVIEW M4 tasks done
- [ ] Demo: sign in → onboarding → Today view → correct a session → CSV export

### M5 — Hardening & validation
- [ ] AGENT M5 tasks done
- [ ] WAREHOUSE M5 tasks done
- [ ] ENGINE M5 tasks done
- [ ] OVERVIEW M5 tasks done
- [ ] End-to-end test suite on Docker Compose green

**Phase 1 exit criteria**
- [ ] Within ±15% of manual baseline for one week
- [ ] ≥ 80% automatic assignment
- [ ] Tenant isolation tests pass
- [ ] Redaction audit passes

## Phase 2 — Collectors & AI

- [ ] SHARED Phase 2 tasks done
- [ ] AGENT Phase 2 tasks done
- [ ] WAREHOUSE Phase 2 tasks done
- [ ] ENGINE Phase 2 tasks done
- [ ] OVERVIEW Phase 2 tasks done

**Exit criteria**
- [ ] Within ±10% of baseline over two weeks
- [ ] ≥ 90% automatic assignment
- [ ] Daily review < 2 minutes

## Phase 3 — Friends & Teams

- [ ] SHARED Phase 3 tasks done
- [ ] AGENT Phase 3 tasks done
- [ ] WAREHOUSE Phase 3 tasks done
- [ ] ENGINE Phase 3 tasks done
- [ ] OVERVIEW Phase 3 tasks done

**Exit criteria**
- [ ] A team of 3–5 uses it for 2 weeks
- [ ] No member can access another member's raw data (tests + review)
- [ ] Submissions match members' own reports

## Phase 4 — Billing & Enterprise

- [ ] ENGINE Phase 4 tasks done
- [ ] OVERVIEW Phase 4 tasks done

**Exit criteria**
- [ ] A full monthly team timesheet or invoice produced without spreadsheets

## Open Decisions

- [ ] OD1 — Own presence watcher vs. ActivityWatch reuse (by M2) · default: build own
- [ ] OD2 — Managed hosting provider/region (by M5) · default: single VPS + Compose
- [ ] OD3 — Tray implementation (by M2) · default: `tray-icon` crate
- [ ] OD4 — Rust → TS type generation (by M1) · default: `ts-rs`
- [ ] OD5 — Job queue implementation (by M3) · default: custom SKIP LOCKED
- [ ] OD6 — Non-coding time handling (by Phase 2) · default: ignored

## Log

| Date | Change |
|---|---|
| 2026-11-06 | Progress files created |
| 2026-11-06 | M0 started: Cargo workspace, CI workflow, agent daemon skeleton (AG-1) |
| 2026-11-06 | Phase 0: `secretary-agent probe` (AG-0.1) works on Windows; macOS/Linux pending manual runs |
| 2026-11-06 | Warehouse M0 done (WH-1–3): Axum skeleton, migrations 0001–0009 with RLS, RLS-scoped DB layer, Docker Compose |
| 2026-11-07 | Decision D14: Keycloak for user and device auth; WH-4, AG-2, OV-2, OV-4, OV-44, SH-7 rescoped |
| 2026-11-07 | `orm/`: single-database Python ORM (`secretary-orm`) extracted from J-ERP; 61 unit + 4 Postgres integration tests pass; not wired into any service (no task ID) |
| 2026-11-07 | Decision D15: warehouse (and later overview API) in Python on `secretary-orm`, ORM owns the schema; WH-1R done (Compose healthy, RLS tests pass) |
| 2026-11-14 | M1 started: `shared` crate (SH-2, SH-3), warehouse device API (WH-4, WH-5, WH-8), agent Keycloak login + SQLite buffer + uploader + heartbeat + presence collector (AG-2, AG-3, AG-5 done; AG-4, AG-8 in progress). M1 demo passes on Windows |
| 2026-11-20 | M1: validation (SH-4) and `AgentConfig` + Appendix A defaults (SH-8) in `shared`; `GET /v1/agent/config` with ETag (WH-9); agent config fetch/cache, exclusions and pause windows (AG-7); pipeline without redaction (AG-6 in progress, needs SH-5) |
| 2026-11-20 | Decision D16: Android app (collector + companion) in Phase 2, no iOS; tasks AG-27–29, SH-23, WH-24, EN-28, OV-24 added |
