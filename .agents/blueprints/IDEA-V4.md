# Cross-Platform AI Work Time Tracker — Version 4 (Cloud, Team-Ready)

> **Status:** Draft blueprint · **Supersedes:** IDEA-V3.md
>
> **What changed from V3:** local-first desktop architecture → **local agent + cloud platform** (ingest, warehouse, engine, overview). Designed **multi-user from day one** so it can be offered to friends and work teams, while the first release serves a single user.
>
> **Structure:** Part I — Product · Part II — System Design · Part III — Delivery · Appendices

---

# Part I — Product

## 1. Summary

A lightweight **agent** on each developer machine collects activity evidence and uploads it to a **cloud platform**. In the cloud, a **processing engine** correlates activity with projects and deterministically reconstructs work sessions; a web **overview dashboard** shows time and cost per project, lets users correct results, and — for teams — shares approved project time.

Time is calculated by a **deterministic, explainable engine**. AI is an optional assistant that classifies ambiguous activity and writes summaries; it never decides durations.

**Core loop:**

**Collect → Upload → Store → Correlate → Calculate → Correct → Share → Report**

1. **Collect** activity events on the machine (presence, editor, Git, terminal, browser, file system).
2. **Upload** redacted, batched events to the cloud.
3. **Store** them immutably in the warehouse.
4. **Correlate** each event with a project — rules first, AI only for leftovers.
5. **Calculate** work sessions deterministically.
6. **Correct** — the user reviews and adjusts; corrections become rules.
7. **Share** — in a team, the user submits project time; only submitted totals become visible to others.
8. **Report** time and cost per project, per person, per team.

## 2. Problem and Target Users

### 2.1 Problem

Developers who switch between several projects per day cannot reliably answer *"how much time did I spend on project X?"* Teams have the same problem multiplied: timesheets are filled from memory, late, and inaccurately.

### 2.2 Target Users (in rollout order)

| Stage | User | Need |
|---|---|---|
| **1. Me** | Single developer, several projects | Accurate time per project without timers |
| **2. Friends** | Independent developers, each with their own projects | Same as above; separate accounts, no shared data |
| **3. Work team** | Developers in one company sharing projects | Accurate, low-effort team timesheets per project, **without surveillance** |

### 2.3 Current Workarounds

- Start/stop timers (Toggl, Clockify) — rely on discipline.
- Editor-only trackers (WakaTime) — miss browser, terminal, and reading time.
- Weekly timesheets reconstructed from Git log, chat, and memory.

## 3. Goals, Non-Goals, and Success Metrics

### 3.1 Goals

- Automatically attribute working time to the correct project across applications and devices.
- Make every calculated minute **explainable** and **correctable**.
- Support individuals and teams with **strict privacy between people**: raw activity belongs to the person who produced it.
- Produce billing- and timesheet-ready reports.
- Be **self-hostable**, so a company can run it on its own infrastructure.

### 3.2 Non-Goals

- **Not an employee surveillance tool.** Managers and admins never see raw activity (window titles, URLs, files, commands) of other members.
- No screenshots, screen recording, clipboard capture, or keystroke content.
- No collection of file contents.
- No productivity scoring or ranking of people.
- AI never invents or decides durations.

### 3.3 Success Metrics

| Metric | Target |
|---|---|
| Accuracy vs. manual log (2-week trial) | Within **±10%** per project per week |
| Automatic assignment rate (after initial mappings) | **≥ 90%** of active time |
| Daily review effort | **< 2 minutes** per user |
| Agent overhead | **< 1% CPU** average, **< 80 MB RAM** |
| Upload latency (online) | Events visible in the dashboard within **5 minutes** |
| Secret leakage (redaction audit) | **Zero** secrets in stored data |
| Cross-tenant data leakage | **Zero** (automated isolation tests) |

## 4. Positioning

| Tool | Strength | Gap this project addresses |
|---|---|---|
| **WakaTime** | Mature editor plugins, cloud dashboard | Editor-centric; weak correlation of browser/terminal work; no evidence-backed timesheet approval |
| **ActivityWatch** | Open-source local watchers | Apps and windows, not projects; no teams |
| **RescueTime / Timing** | Automatic, polished | Productivity categories; surveillance perception in teams |
| **Toggl / Clockify / Harvest** | Team timesheets and billing | Manual timers |
| **Hubstaff / Time Doctor** | Automatic team tracking | Screenshots and monitoring — the opposite of our privacy model |

**Differentiator:** automatic, cross-application **project** time with explainable evidence, and a **private-until-submitted** team model — automatic like a tracker, respectful like a timesheet.

## 5. User Experience

### 5.1 Onboarding — Individual (~5 minutes)

1. Sign up on the web with GitHub or Google. A **personal workspace** is created.
2. Download and install the agent. Sign in through the browser (Keycloak device authorization flow); the agent keeps a refresh token in the OS keychain.
3. Grant OS permissions, each with a plain-language explanation.
4. Choose project roots (e.g., `~/code`). The agent detects repositories; the dashboard proposes one project per repository; the user merges, renames, or ignores.
5. Install the VS Code extension and browser extension (both talk to the local agent, not directly to the cloud).
6. Optional: **backfill** the last 4 weeks from Git history (marked *estimated*).
7. Choose AI mode: **Off** (default) · **On**.

### 5.2 Onboarding — Team

1. A user creates a **team workspace** and invites members by email or invite link.
2. The owner defines **shared projects** (e.g., "Client Acme", "Internal Platform") with optional repository/domain mappings.
3. Members join, then link their local repositories to shared projects (or accept suggested matches by remote URL).
4. Each member keeps a personal workspace too; personal projects never appear in the team.

### 5.3 A Normal Day

- The agent runs silently; the tray shows current project, today's total, and sync status.
- One click: **Pause tracking** (15 min / 1 h / until resumed) or **Mark as private**.
- Works offline; events upload when connectivity returns.

### 5.4 End-of-Day Review (< 2 minutes)

1. Dashboard → **Today** timeline: one lane per project, across all the user's devices.
2. **Unassigned inbox** lists activity that could not be attributed confidently; assign in bulk.
3. After a correction, the app proposes a rule: *"Always assign `github.com/acme/*` to Acme?"*
4. Any session opens to show **why it exists** (evidence) and can be trimmed, split, reassigned, or deleted.

### 5.5 Team Timesheet Flow

1. At the end of a day or week, the member clicks **Submit** for team projects.
2. Only **submitted project totals** (and optional summaries the member approves) become visible to the team.
3. A team admin may **approve** or **request changes**, but cannot edit or view the member's raw activity.
4. Submitted entries are locked; later changes create a visible revision.

### 5.6 Reports

- **Individual:** time per project, active vs. waiting time, cost, AI summary (optional).
- **Team:** submitted time per project, per member, per period; budget vs. actual.
- Export CSV/Excel; integrations in later phases.

### 5.7 Privacy Controls (always available)

- See exactly what was collected, per event, per device.
- Delete any event, day, device, project, or the whole account.
- Manage exclusions (directories, apps, domains, time windows).
- See what, if anything, was sent to AI.
- Revoke any device.

---

# Part II — System Design

## 6. Architecture Overview

```
 ┌──────────────────────── Developer machine ───────────────────────────┐
 │  VS Code Ext   Browser Ext   Shell Hooks                             │
 │       │   (localhost / Native Messaging)  │                          │
 │       ▼                ▼                  ▼                          │
 │  ┌─────────────────────── agent/ (Rust) ───────────────────────────┐ │
 │  │ Activity & Focus Watcher · Git Tracker · FS Tracker             │ │
 │  │ Redaction · Exclusions · Local SQLite buffer · Uploader         │ │
 │  └───────────────────────────────┬─────────────────────────────────┘ │
 └──────────────────────────────────┼───────────────────────────────────┘
                                    │ HTTPS, gzip batches, access token
                                    ▼
 ┌────────────────────────────── Cloud ─────────────────────────────────┐
 │  warehouse/                                                          │
 │   ├─ Ingest API ──► Raw archive (object storage, append-only)        │
 │   │             └─► Postgres: activity_events (immutable)            │
 │   │                       │  enqueue (user, local day)               │
 │   ▼                       ▼                                          │
 │  engine/  Correlation ──(ambiguous)──► AI provider                   │
 │           Time calculation ──► sessions, evidence (Postgres)         │
 │                                   │                                  │
 │  overview/  API + Web dashboard ◄─┘   (auth, workspaces, RLS)        │
 └──────────────────────────────────────────────────────────────────────┘
```

### 6.1 Components and Repository Layout

| Folder | Component | Responsibility |
|---|---|---|
| `agent/` | **Agent** (Rust daemon + tray) | Collect, redact, buffer, upload. Hosts presence/focus, Git, FS collectors; receives events from VS Code, browser, shell. |
| `agent/vscode/` | VS Code extension (TypeScript) | Editor events → local agent |
| `agent/browser/` | Browser extension (WebExtensions) | Tab focus events → local agent via Native Messaging |
| `agent/shell/` | Shell hooks | Command events → local agent |
| `warehouse/` | **Ingest API + storage** | Authenticate devices, validate and store events immutably, archive raw batches, enqueue processing |
| `engine/` | **Processing engine** (Rust worker) | Correlation, time calculation, AI calls, recalculation |
| `overview/` | **Dashboard** (API + React web app) | Auth, workspaces, projects, review and corrections, team submissions, reports |
| `shared/` | Shared Rust crate | Event schema, validation, redaction rules — used by agent, warehouse, and engine |

### 6.2 Principles

- **Raw is immutable; everything else is derived.** Assignments, sessions, and reports can be recomputed.
- **Deterministic engine.** Same inputs + rules + algorithm version → same output.
- **Privacy at the edge.** Redaction and exclusions happen in the agent, before upload.
- **Private by default between people.** Raw activity is visible only to its owner.
- **Multi-tenant from day one.** Every row is scoped to a workspace and enforced by the database.
- **The dashboard does not compute.** It reads results from the engine and writes corrections; it never recalculates time on page load.
- **Self-hostable.** The full cloud side runs from one Docker Compose file.

## 7. Glossary / Domain Model

| Term | Definition |
|---|---|
| **User** | A person with an account. |
| **Workspace** | A tenant boundary. Every user has a **personal workspace**; **team workspaces** group members around shared projects. |
| **Member** | A user's membership in a workspace, with a role (§14.3). |
| **Device** | A machine running the agent, registered to one user, with its own revocable token. |
| **Event** | An immutable record of one observed activity, owned by one user and device. |
| **Presence** | Whether the user is actively using the device, from input recency. |
| **Idle** | No input for at least the idle threshold. |
| **Project** | A unit time is reported against. Personal projects live in a personal workspace; shared projects live in a team workspace. |
| **Mapping** | A deterministic rule linking a signal (path, remote, domain, title pattern) to a project. Can be personal or shared. |
| **Assignment** | Link between one event and one project, with confidence and method. |
| **Session** | A continuous derived interval of work on one project by one user. |
| **Evidence** | Events that justify a session. Visible only to the session owner. |
| **Active time** | Session time while the user is present. |
| **Waiting time** | A project's background task running while the user is idle. |
| **Adjustment** | A manual correction to sessions, reapplied after recalculation. |
| **Submission** | A user's approved project totals for a period, shared with a team workspace. |
| **Coverage gap** | Period when a collector or device was offline; absence of events is not idleness. |
| **Local day** | A calendar day in the user's configured time zone; the unit of processing and review. |

## 8. Agent and Collectors

### 8.1 Agent Responsibilities

- Host the Activity & Focus, Git, and File System collectors.
- Receive events from the VS Code extension, browser extension, and shell hooks (localhost with per-collector tokens, or Native Messaging).
- Apply **redaction and exclusions** (rules synced from the cloud, enforced locally).
- **Buffer** events in a local SQLite queue (bounded, e.g., 30 days).
- **Upload** gzip batches over HTTPS every 60 s (or on 500 events), with retry and backoff.
- Report **clock offset** (device clock vs. server time) with each batch.
- Send **heartbeats** for coverage-gap detection.
- Auto-update via signed releases.

### 8.2 Collector Contract

- Redact at source; send only fields defined in the schema.
- Generate **UUIDv7** event IDs for idempotent ingestion.
- Respect pause state and exclusions.
- Stay lightweight; never block the editor, shell, or browser.

### 8.3 Collector Specifications

| # | Collector | Runs in | Signals | Phase |
|---|---|---|---|---|
| 1 | **Activity & Focus Watcher** | Agent | Input idle state (no key content), focused app, window title | 1 — required |
| 2 | **VS Code Extension** | VS Code → agent | Workspace, active file, edit bursts, saves, debug sessions, integrated terminal | 1 |
| 3 | **Git Tracker** | Agent | Repos, remote URL, branches, commits, checkouts, merges; backfill | 1 |
| 4 | **Terminal Tracker** | Shell hooks → agent | Command name, cwd, start/end, exit code | 2 |
| 5 | **Browser Extension** | Browser → agent | Redacted URL, title, focused tab duration | 2 |
| 6 | **File System Tracker** | Agent | Create/modify/delete/rename in project roots | 2 — optional |

**Notes:**

- **Activity & Focus Watcher** is mandatory: idle detection and reading time are impossible without it, and it can only run on the device.
- **VS Code Extension** aggregates edits into bursts (e.g., per 30 s) and tags integrated-terminal commands for deduplication.
- **Git Tracker** sends repository **remote URL** — the key signal for matching a member's local repo to a shared team project.
- **Terminal Tracker** stores the **command name only** by default; hooks for Bash, Zsh, Fish, PowerShell.
- **Browser Extension** tracks focused tab only, strips query strings and fragments, never tracks private windows.
- **File System Tracker** watches project roots only and ignores `.git/`, `node_modules/`, `target/`, `dist/`, `build/`, `.venv/`, and `.gitignore`d paths.

## 9. Ingestion and Event Schema (`warehouse/`)

### 9.1 Ingest API

- `POST /v1/events:batch` — gzip JSON (or NDJSON) batch, max e.g. 1,000 events / 1 MB.
- Authenticated with a short-lived **Keycloak access token**; its session (`sid`) maps to exactly one registered device, revocable from the dashboard (D14).
- **Idempotent** on `event_id`; duplicates are acknowledged and discarded.
- Validates against `schema_version`; rejects invalid events with per-event errors.
- Stamps `received_at` and applies the reported **clock offset** to produce `server_adjusted_start/end`.
- Writes the raw batch to **object storage** (append-only archive) and the events to **Postgres**.
- Enqueues processing jobs for each affected **(user, local day)**.
- Rate-limited per device.

### 9.2 Event Schema

| Field | Type | Description |
|---|---|---|
| `event_id` | UUIDv7 | Generated by the collector |
| `schema_version` | int | Event schema version |
| `user_id` | UUID | Set by the server from the access token (never trusted from payload) |
| `device_id` | UUID | Set by the server from the access token's session |
| `timestamp_start` | UTC datetime | Device-reported start |
| `timestamp_end` | UTC datetime, nullable | End for interval events |
| `server_adjusted_start` / `_end` | UTC datetime | Corrected for clock offset |
| `received_at` | UTC datetime | Server receive time |
| `source` | string | Collector name and version, e.g. `vscode@0.3.1` |
| `event_type` | string | See Appendix B |
| `project_hint` | UUID, nullable | Project if the collector knows it |
| `repository` | string, nullable | Repo root (path) and/or normalized remote URL |
| `working_directory` | string, nullable | Working directory |
| `metadata` | JSONB | Event-specific payload, already redacted |

### 9.3 Storage Choice

- **Postgres** is the system of record (events, assignments, sessions, workspaces). Sized estimate: ~5,000 events/user/day × ~500 B ≈ **2.5 MB/user/day ≈ 1 GB/user/year** — comfortably within Postgres for teams of tens to hundreds.
- **Object storage** (S3-compatible: AWS S3, Cloudflare R2, MinIO for self-hosting) keeps raw batches as an immutable archive for audit and replay.
- `activity_events` is **partitioned by month**.
- A dedicated analytical warehouse (BigQuery, ClickHouse, etc.) is **not needed** now; it can be added later fed from the archive if analytics grows.

## 10. Project Correlation (`engine/`)

### 10.1 Resolution Order

| Step | Method | Confidence |
|---|---|---|
| 1 | **User override** — manual assignment | 1.0 |
| 2 | **Personal mapping rule** | 0.95 |
| 3 | **Shared (team) mapping rule** — e.g., remote `github.com/acme/*` → "Client Acme" | 0.95 |
| 4 | **Structural signal** — repo root, VS Code workspace, terminal cwd | 0.9 |
| 5 | **Temporal context** — surrounded by one project's events within the context window | 0.6–0.8 |
| 6 | **AI classification** — only if still ambiguous and AI is enabled | ≤ 0.8 |
| 7 | **Unassigned** — review inbox | — |

Personal rules override shared rules. Events below the **review threshold** (default 0.7) appear in the inbox.

### 10.2 Assignment Record

`event_id`, `user_id`, `project_id`, `confidence`, `method` (`user` | `rule` | `shared_rule` | `structural` | `temporal` | `ai`), `rule_id` or `model_id`, `created_at`.

### 10.3 Learning From Corrections

Corrections with a shared pattern produce **rule proposals**. Accepted proposals become personal rules; a team admin may promote a rule to a shared rule. Rejected proposals are not repeated.

## 11. Time Calculation (`engine/`)

### 11.1 Processing Model

- Jobs are keyed by **(user_id, local_day)** and queued on ingestion, on rule/assignment changes, and on algorithm upgrades.
- Jobs are **idempotent**: a job recomputes the whole local day for that user and replaces derived rows transactionally.
- Debounced (e.g., 2 minutes) so a burst of uploads triggers one recalculation.
- Queue implemented in Postgres (e.g., `SELECT … FOR UPDATE SKIP LOCKED` or pgmq) — no extra infrastructure.
- Each derived session records `algorithm_version` and `config_hash`.

### 11.2 Algorithm (per user, per local day, across all devices)

1. **Build presence timeline** per device from the Activity & Focus Watcher (present vs. idle ≥ idle threshold), then merge devices: the user is present if present on any device.
2. **Credit events** per event type (Appendix A). Interval events use their actual duration.
3. **Clip to presence:** drop credited time inside idle periods — except background tasks, which become **waiting time** candidates.
4. **Resolve overlaps:** one project at a time per user, chosen by:
   1. The device with the most recent input
   2. On that device: focused window → VS Code workspace → terminal → browser → file system
5. **Merge** same-project intervals separated by gaps ≤ merge gap (if present).
6. **Drop** sessions shorter than the minimum length.
7. **Flag coverage gaps** as low-confidence rather than silently counting or dropping.
8. **Apply adjustments** from `time_adjustments`.

### 11.3 Waiting Time

- Long-running commands while the user is idle become **waiting time**, reported separately.
- Each project defines whether waiting time is billable.
- Background file changes with no presence are ignored.

### 11.4 Explainability and Recalculation

- Every session links to its evidence (visible only to its owner).
- Recalculation touches only affected (user, day) pairs.
- Adjustments are anchored to time ranges and projects, so they survive recalculation.
- **Submitted** days are never changed silently: if recalculation changes a submitted total, the user is notified to resubmit.

## 12. AI Layer (`engine/`)

### 12.1 Uses

| Use | Phase |
|---|---|
| Classify ambiguous events | 2 |
| Associate browser research with the active project | 2 |
| Suggest mapping rules from corrections | 2 |
| Daily/weekly summaries per project (owner-only unless submitted) | 2 |
| Draft timesheet/invoice line descriptions | 4 |

### 12.2 Rules

- **Never decides durations.**
- **Grounded:** every summary statement references supporting events.
- **Cached** by input hash; regeneration is stable.
- **Minimal input:** domain + title, repo-relative paths, command names; never file contents; never another user's data.
- **Batched** and **budgeted** (per user and per workspace); when over budget, events stay unassigned.
- **Policy levels:** user setting (Off/On) and team workspace policy (Allowed / Disabled / Specific provider only). The stricter wins.
- AI requests are logged (`ai_request_log`) and visible to the user.
- Provider is configurable; self-hosted deployments may point to a local model server.

## 13. Data Model and Storage

All tables carry `workspace_id` and/or `user_id`; access is enforced with **Postgres Row-Level Security**.

| Entity | Kind | Scope | Purpose |
|---|---|---|---|
| `users` | Identity | Global | Accounts (OAuth identities) |
| `workspaces` | Tenancy | Global | Personal and team workspaces |
| `memberships` | Tenancy | Workspace | User ↔ workspace with role |
| `invitations` | Tenancy | Workspace | Pending invites |
| `devices` | Identity | User | Registered agents, hashed tokens, last seen, clock offset |
| `activity_events` | Raw | User | Immutable events (partitioned by month) |
| `projects` | Config | Workspace | Projects, rate, billable and waiting-time policy, budget |
| `project_links` | Config | User | A user's link from personal repos/paths to a shared project |
| `project_mappings` | Config | User or workspace | Personal or shared rules |
| `rule_proposals` | Config | User | Suggested rules and state |
| `exclusions` | Config | User | Excluded directories, apps, domains, time windows |
| `event_assignments` | Derived | User | Event → project |
| `ai_classifications` | Cache | User | Cached AI results |
| `ai_request_log` | Audit | User | What was sent to AI, when, to which provider |
| `work_sessions` | Derived | User | Sessions (active/waiting), algorithm version, config hash |
| `session_evidence` | Derived | User | Session → events |
| `time_adjustments` | User data | User | Manual corrections |
| `coverage_gaps` | Derived | User + device | Offline periods |
| `submissions` | Shared | Workspace | Submitted totals per user, project, period; status, revisions |
| `audit_log` | Audit | Workspace | Admin and security actions |
| `processing_jobs` | System | — | Engine queue |

- Reports are **views or caches** over sessions and submissions.
- **Retention** (configurable per workspace): raw events 12 months, then compacted to sessions + evidence summaries; raw archive lifecycle rules in object storage.
- Versioned schema migrations.

## 14. Privacy and Security

### 14.1 Visibility Model

| Data | Owner | Team members | Team admin | Platform operator |
|---|---|---|---|---|
| Raw events, evidence, titles, URLs, commands | ✅ | ❌ | ❌ | ❌ by policy (encrypted fields, §14.4) |
| Personal projects and their time | ✅ | ❌ | ❌ | ❌ |
| Draft (unsubmitted) shared-project time | ✅ | ❌ | ❌ | ❌ |
| **Submitted** shared-project totals | ✅ | ✅ (configurable) | ✅ | — |
| Approved AI summaries attached to a submission | ✅ | ✅ (configurable) | ✅ | — |
| Device list | ✅ | ❌ | ❌ | — |

**Principle: private until submitted.** The team sees what each member chooses to submit — never how they spent the day.

### 14.2 Threat Model

| Threat | Mitigation |
|---|---|
| Secrets in terminal commands or URLs | Command-name-only default; strip query strings; secret-pattern redaction in the agent; CI redaction audit |
| Personal browsing captured | Focused tab only; never private windows; domain allow/deny lists; time-window exclusions |
| Cross-tenant data leakage | Postgres RLS on every table; tenant ID never taken from client payload; automated isolation tests in CI |
| Manager overreach / surveillance | Visibility model (§14.1) enforced in the database; no admin "view as" for raw data; audit log of admin actions |
| Stolen agent token | Refresh token scoped to one device session; short-lived access tokens; refresh-token rotation; revocable from dashboard; anomaly detection (new IP/region) |
| Fake events from local malware or websites | Agent's local endpoints bound to localhost with per-collector tokens; strict CORS; Native Messaging for browser |
| Server or database breach | TLS everywhere; encryption at rest; application-level encryption of sensitive metadata (§14.4); minimal data by design |
| Data sent to AI provider | Off by default; user and workspace policy; minimal redacted fields; request log |
| Data residency / compliance | Region choice for hosted version; full **self-hosting** option for companies |

### 14.3 Roles

| Role | Can |
|---|---|
| **Owner** | Everything an admin can; delete workspace; transfer ownership |
| **Admin** | Manage members, shared projects, shared rules, AI policy, retention; approve submissions; view submitted reports |
| **Member** | Link repos to shared projects; submit time; view submitted reports (if allowed) |
| **Viewer** (optional) | View submitted reports only (e.g., finance/PM) |

### 14.4 Encryption

- TLS 1.2+ for all traffic; HSTS on the dashboard.
- Disk/database encryption at rest (provider-managed or self-hosted).
- **Application-level encryption** of sensitive metadata fields (window titles, URLs, file paths, command names) with a per-user data key (envelope encryption via a KMS). The engine decrypts in memory only while processing.
- API keys stored hashed; agent tokens are issued and stored by Keycloak, never by our services.

### 14.5 Compliance and Workplace Use

- **User rights:** export all personal data (JSON/CSV) and delete account and data.
- **Workplace deployments** may be subject to employee-monitoring laws (e.g., GDPR in the EU, works-council agreements in some countries). The product supports this by design — private-until-submitted, no surveillance features, transparent collection — but each company remains responsible for informing members and obtaining required approvals.
- Clear in-app disclosure of what is collected and who can see it, shown when joining a team.

## 15. Platform Support Matrix (Agent)

| Collector | Windows | macOS | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| Idle detection | ✅ `GetLastInputInfo` | ✅ `CGEventSource` | ✅ XScreenSaver | ⚠️ Compositor-specific (`ext-idle-notify`) |
| Focused window / title | ✅ `GetForegroundWindow` | ⚠️ Accessibility permission | ✅ `_NET_ACTIVE_WINDOW` | ⚠️ Limited; GNOME/KDE extensions |
| VS Code extension | ✅ | ✅ | ✅ | ✅ |
| Git tracker | ✅ | ✅ | ✅ | ✅ |
| Terminal hooks | ✅ PowerShell, Git Bash | ✅ Zsh, Bash, Fish | ✅ Bash, Zsh, Fish | ✅ Bash, Zsh, Fish |
| Browser extension | ✅ Chromium, Firefox | ✅ Chromium, Firefox | ✅ Chromium, Firefox | ✅ Chromium, Firefox |
| File system watcher | ✅ `ReadDirectoryChangesW` | ✅ FSEvents | ✅ inotify | ✅ inotify |

✅ supported · ⚠️ best-effort / requires permission or extension

**Android (Phase 2, D16):** one app, `agent/android/`, that is both a collector and a companion. iOS is not supported: it gives apps no access to other apps' usage.

| Signal / feature | Android | How |
|---|---|---|
| Presence (screen on and unlocked) | ✅ | `UsageStatsManager` events (`SCREEN_INTERACTIVE`, `KEYGUARD_HIDDEN` / `SHOWN`) → `presence.active` / `presence.idle` |
| Foreground app | ⚠️ needs the "Usage access" permission | `UsageEvents` (`ACTIVITY_RESUMED` / `PAUSED`) → `window.focus` with `app` = package name and **no title** |
| Window titles, notifications, screen content | ❌ never | No accessibility service, notification listener or screen capture |
| Editor, Git, terminal, browser collectors | ❌ | Desktop only |
| Companion | ✅ | Today's total, sessions to review, pause / resume tracking on this phone |

## 16. Technology Stack and Key Decisions

### 16.1 Stack

| Component | Technology |
|---|---|
| Agent (daemon + tray) | Rust; tray via Tauri or a native tray crate |
| Agent local buffer | SQLite |
| VS Code extension | TypeScript, VS Code Extension API |
| Browser extension | WebExtensions + Native Messaging |
| Shell hooks | Bash, Zsh, Fish, PowerShell |
| Shared schema & redaction | Rust crate (`shared/`), compiled into agent, ingest, and engine |
| Ingest API | Python (FastAPI + uvicorn) on `secretary-orm` (D15) |
| Processing engine | Rust worker |
| System of record | PostgreSQL (with RLS, monthly partitions) |
| Job queue | Postgres-based (SKIP LOCKED / pgmq) |
| Raw archive | S3-compatible object storage (S3, R2, MinIO) |
| Dashboard API | Python (FastAPI) sharing the warehouse's ORM models (D15) |
| Dashboard UI | React + TypeScript (Vite) |
| Auth | **Keycloak** (OIDC) for all sign-in: dashboard (auth code + PKCE), agent (OAuth 2.0 device authorization grant, offline token), GitHub/Google via identity brokering, company SSO (OIDC/SAML) in Phase 4. Services validate Keycloak JWTs (D14) |
| Secrets / keys | KMS (cloud) or Vault/age-based keys (self-hosted) |
| Deployment | Containers; Docker Compose for self-hosting; any container host for the managed version |
| Observability | OpenTelemetry traces/metrics; structured logs (never containing event metadata) |

### 16.2 Decision Log

| # | Decision | Rationale |
|---|---|---|
| D1 | Deterministic calculation; AI never sets durations | Timesheets and billing need reproducible, defensible numbers |
| D2 | Raw events immutable; all else derived | Recalculation when rules or algorithms improve |
| D3 | **Local agent + cloud platform** (replaces local-first) | Multi-device and team use; heavy processing and AI off the laptop |
| D4 | **Multi-tenant from day one** (workspaces + RLS) | Retrofitting tenancy later is expensive and risky; first release still single-user |
| D5 | **Private until submitted** | Makes team adoption acceptable; avoids surveillance; reduces legal exposure |
| D6 | Redaction in the agent | Sensitive data never leaves the device unredacted |
| D7 | Postgres, not a data warehouse | Volume is small (~1 GB/user/year); one database is simpler; add analytics store later if needed |
| D8 | Postgres-based job queue | No extra infrastructure until scale demands it |
| D9 | Rust for the agent and engine, with a shared crate (amended by D15) | One event schema and redaction implementation; no drift between components |
| D10 | Dashboard never computes time | Results are stored, explainable, and identical for every viewer |
| D11 | Self-hostable via Docker Compose | Companies can keep data on their own infrastructure |
| D12 | Browser and editor extensions talk to the local agent, not the cloud | One authenticated upload path; redaction and buffering in one place |
| D13 | AI off by default; stricter of user and workspace policy wins | Privacy-first; product must work on rules alone |
| D15 | **Warehouse and dashboard API in Python on `secretary-orm`; the ORM owns the schema** (2026-11-07) | One model definition for every Python service, written in the owner's own ORM (from J-ERP); tables, columns, indexes, FKs and checks come from the models, while RLS policies, grants, partitions and SECURITY DEFINER functions stay in idempotent SQL run in the same migration transaction. The event contract reaches Python as JSON Schema generated from `shared/`. Cost: two backend languages; bigint ids instead of UUIDs (agent event IDs stay UUIDv7) |
| D16 | **Android app (Phase 2): collector + companion; no iOS** (2026-11-20) | Phone work (reviewing PRs, chat, docs) is part of project time, and Android exposes app usage through `UsageStatsManager`. iOS has no equivalent. Kotlin, not the Rust daemon: the Android APIs are JVM-first, and the event contract comes from the SH-9 JSON Schema. Sign-in uses Keycloak authorization code + PKCE (client `secretary-android`); the device grant is for devices without a convenient browser |
| D14 | **Keycloak is the identity provider for users and devices** (2026-11-07) | One standard auth system (OIDC, device grant, MFA, social login, enterprise SSO) instead of custom device tokens; self-hosters can reuse it. Cost: an extra JVM service in Compose, token refresh in the agent. Authorization (workspaces, roles, RLS) stays in Postgres |

---

# Part III — Delivery

## 17. Roadmap

### Phase 0 — Validation (1–2 weeks)

- Prototype the Activity & Focus Watcher on Windows, macOS, Linux.
- Evaluate reusing ActivityWatch watchers inside the agent.
- Choose hosting for the managed version.
- Keep a manual time log for 2 weeks as the accuracy baseline.

**Exit criteria:** watcher prototype on all three OSes; hosting chosen; baseline log collected.

### Phase 1 — Personal MVP (single user, multi-tenant schema)

- `shared/`: event schema, validation, redaction.
- `agent/`: Activity & Focus Watcher, Git tracker with backfill, local buffer, uploader, device login, pause, exclusions.
- `agent/vscode/`: VS Code extension.
- `warehouse/`: ingest API, Postgres with workspaces + RLS, raw archive.
- `engine/`: rule-based correlation, time calculation, job queue.
- `overview/`: GitHub/Google login, personal workspace, Today/Week views, timeline, evidence, **manual corrections**, unassigned inbox, projects and mappings, devices, CSV export, data export/delete.
- Multi-device works naturally (one user, several devices).

**Exit criteria:** within **±15%** of baseline for one week; ≥ 80% automatic assignment; isolation tests pass; redaction audit passes.

### Phase 2 — More Collectors and AI

- Terminal tracker, browser extension, file system tracker.
- Android app (D16): presence and foreground-app collector, plus companion (today's total, review, pause).
- AI layer: classification, research association, rule suggestions, summaries; user AI setting.
- Collector/device health and coverage-gap views.
- Application-level encryption of sensitive metadata.

**Exit criteria:** within **±10%** of baseline over two weeks; ≥ 90% automatic assignment; daily review < 2 minutes.

### Phase 3 — Friends and Teams

- Invite-only sign-up for friends (separate personal workspaces).
- Team workspaces: invitations, roles, shared projects, shared rules, project links by remote URL.
- Submission and approval flow; team reports.
- Workspace AI policy and retention settings.
- Audit log.
- **Self-hosting** package (Docker Compose + docs).
- Join-team privacy disclosure.

**Exit criteria:** a team of 3–5 uses it for 2 weeks; no member can access another member's raw data (verified by tests and review); submissions match members' own reports.

### Phase 4 — Billing, Integrations, Enterprise

- Hourly rates, billable/non-billable, waiting-time policy, rounding rules.
- Budgets: estimated vs. actual per project.
- Excel export; integrations (Jira, Harvest, Toggl, Clockify).
- AI-drafted timesheet/invoice descriptions.
- OIDC/SSO for companies.

**Exit criteria:** a full monthly team timesheet or invoice can be produced without spreadsheets.

## 18. Risks and Open Questions

### 18.1 Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Team members perceive it as surveillance | Adoption fails | Private-until-submitted; no raw access for admins; transparent disclosure; no productivity scores |
| Employee-monitoring regulations | Legal blockers in companies | Privacy model by design; self-hosting; documentation for HR/legal review |
| Cross-tenant leakage bug | Severe trust loss | RLS everywhere; tenant from token only; isolation test suite; security review before Phase 3 |
| Under-counting thinking/reading time | Low accuracy | Presence + focus; tunable merge gap; baseline validation |
| Wayland / macOS permission limits | Reduced accuracy | Best-effort support; graceful degradation; clear onboarding |
| Secret leakage | Trust loss | Command-name-only default; agent-side redaction; audit suite |
| Hosting cost and operations | Burden on a solo builder | Small footprint (one Postgres, one container, object storage); self-hosting shifts cost to teams |
| Clock skew between devices | Misordered events | Clock offset per batch; server-adjusted timestamps |
| Scope creep | Delayed MVP | Phase exit criteria are hard gates; teams only in Phase 3 |

### 18.2 Open Questions

1. Managed hosting provider and region (and whether to offer a managed version at all, or self-host only for teams)?
2. How to handle non-coding work (email, chat, meetings) — ignore, "overhead" project, or calendar integration?
3. Should projects support sub-units (tasks, tickets, branches) for team reporting?
4. Should team members see each other's submitted totals by default, or only admins?
5. Which editors beyond VS Code get native extensions, and when (JetBrains, Neovim)?
6. Licensing and distribution model (open source, paid, freemium)?

---

# Appendices

## Appendix A — Default Configuration Values

All values are configurable (per user; some per workspace) and will be tuned using the Phase 0 baseline.

| Setting | Default | Used in |
|---|---|---|
| Idle threshold | 5 min | §11.2 step 1 |
| Merge gap | 10 min | §11.2 step 5 |
| Minimum session length | 1 min | §11.2 step 6 |
| Temporal context window | 5 min | §10.1 step 5 |
| Review threshold (confidence) | 0.7 | §10.1 |
| Agent upload interval | 60 s or 500 events | §8.1 |
| Agent local buffer limit | 30 days | §8.1 |
| Heartbeat interval | 60 s | §8.1 |
| Engine debounce | 2 min | §11.1 |
| VS Code edit aggregation window | 30 s | §8.3 |
| Raw event retention | 12 months | §13 |
| AI | Off | §12.2 |

**Event credit:**

| Event | Credit |
|---|---|
| Editor edit burst / save | 2 min |
| Editor focus on project file | Actual focus duration while present |
| Terminal command | Actual run duration; idle portion → waiting time |
| Git commit / checkout / merge | 2 min |
| Browser tab focus (assigned) | Actual focus duration while present |
| File system change (no editor signal) | 1 min |
| Git backfill commit (historical) | 30 min before commit, marked *estimated* |

**Overlap priority:** most recently active device → focused window → VS Code workspace → terminal → browser → file system.

## Appendix B — Event Type Catalog

| Event type | Collector | Kind | Key metadata |
|---|---|---|---|
| `presence.active` | Activity | Interval | — |
| `presence.idle` | Activity | Interval | — |
| `window.focus` | Activity | Interval | `app`, `title` (redacted) |
| `editor.workspace_open` | VS Code | Point | `workspace_path`, `remote_url` |
| `editor.file_focus` | VS Code | Interval | `file_path` (repo-relative), `language` |
| `editor.edit_burst` | VS Code | Interval | `file_path`, `edit_count` |
| `editor.save` | VS Code | Point | `file_path` |
| `editor.debug_session` | VS Code | Interval | `configuration_name` |
| `git.commit` | Git | Point | `hash`, `branch`, `files_changed`, `remote_url` |
| `git.checkout` | Git | Point | `from_branch`, `to_branch` |
| `git.merge` | Git | Point | `branch` |
| `git.backfill_commit` | Git | Point | `hash`, `branch`, `estimated: true` |
| `terminal.command` | Terminal | Interval | `command_name`, `exit_code`, `shell`, `from_vscode` |
| `browser.tab_focus` | Browser | Interval | `domain`, `path` (redacted), `title` |
| `fs.change` | File system | Point | `path` (repo-relative), `change_kind` |
| `agent.heartbeat` | Agent | Point | `agent_version`, `queue_depth`, `clock_offset_ms` |
| `agent.pause` / `agent.resume` | Agent | Point | `reason` (`user`, `exclusion_window`) |

## Appendix C — Ingest API Sketch

| Endpoint | Auth | Purpose |
|---|---|---|
| `GET /v1/auth/config` | — | Keycloak issuer URL + agent client ID, so the agent only needs the server URL |
| `POST /v1/devices` | Agent access token | Register this device after the Keycloak device login |
| `POST /v1/events:batch` | Agent access token | Upload a gzip batch of events |
| `GET /v1/agent/config` | Agent access token | Fetch exclusions, redaction rules, pause windows, project roots |
| `POST /v1/agent/heartbeat` | Agent access token | Health, queue depth, clock offset |
| `DELETE /v1/devices/{id}` | User session | Revoke a device (also ends its Keycloak offline session) |

The agent obtains tokens from Keycloak directly (device authorization grant, D14); the warehouse no longer issues device tokens.

---

## Final Goal

A reliable, private, team-ready work time platform: a lightweight agent captures real development evidence on every device, a cloud engine turns it into explainable, correctable project sessions, and each person shares only the project time they choose to submit — producing accurate time-cost reports for individuals and teams without manual timers or surveillance.
