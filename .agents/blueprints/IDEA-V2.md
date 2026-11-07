# Cross-Platform AI Work Time Tracker — Version 2

## 1. Project Overview

Build a cross-platform, local-first background daemon that automatically collects development activity across applications, reconstructs work sessions, and calculates the actual time spent on each project.

The goal is to remove the need to manually calculate project time costs when frequently switching between multiple projects throughout the day.

The system uses activity events as **evidence**, a **deterministic and explainable** time calculation engine, and **AI only as an assistant** for ambiguous classification and summarization.

### 1.1 Problem Statement

Developers who work on several projects per day cannot reliably answer "how much time did I spend on project X?" Manual timers are forgotten, and app-usage trackers measure which application was open, not which project was being worked on.

### 1.2 Differentiation

| Tool | Limitation this project addresses |
|---|---|
| WakaTime | Editor-centric; cloud-first; weak correlation of browser and terminal work to projects. |
| ActivityWatch | Excellent local collection, but tracks apps/windows, not projects; no billing-oriented reports. |
| RescueTime / Timing | Productivity categories rather than per-project, evidence-backed time cost. |

**Our focus:** cross-application *project correlation* + explainable session reconstruction + billing-ready reports, fully local-first.

> Note: ActivityWatch's watchers (window, AFK, browser) are open source. Reusing or interoperating with them is an option to evaluate before building equivalent collectors from scratch.

### 1.3 Success Metrics

- Calculated project time is within **±10%** of a manually kept log over a 2-week trial.
- **≥ 90%** of active time is automatically assigned to a project without manual correction (after initial mappings are configured).
- Daemon overhead: **< 1% CPU** average and **< 100 MB RAM**.
- Zero secrets found in stored data during a redaction audit.

### 1.4 Non-Goals

- Not an employee surveillance or monitoring tool.
- No screenshots, screen recording, or keystroke content logging.
- No collection of full file contents.
- No cloud dependency for core functionality.
- AI never invents durations; it only classifies and summarizes.

## 2. Core Concept

**Collect → Centralize → Correlate → Calculate → Correct → Report**

1. **Collect** activity events from collectors.
2. **Centralize** them in an immutable local event store.
3. **Correlate** each event with a project (rules first, AI for leftovers).
4. **Calculate** work sessions deterministically from evidence.
5. **Correct** — the user reviews, reassigns, and adjusts; corrections become rules.
6. **Report** time and cost per project.

Instead of relying on application usage duration alone, the system combines **presence signals** (is the user at the computer, which window is focused) with **project signals** (which repository, file, command, or URL) to infer working time.

## 3. Activity Collectors

| # | Collector | Signals | Priority |
|---|---|---|---|
| 1 | **Activity & Focus Watcher** | Keyboard/mouse idle state (no key content), focused application, window title | Phase 1 — required |
| 2 | **VS Code Extension** | Workspace, active file, edits, saves, debug sessions, integrated terminal | Phase 1 |
| 3 | **Git Tracker** | Repositories, branches, commits, checkouts, merges; historical backfill | Phase 1 |
| 4 | **Terminal Tracker** | Command name, working directory, start/end, exit code | Phase 2 |
| 5 | **Browser Extension** | URL (redacted), page title, active tab changes, focus duration | Phase 2 |
| 6 | **File System Tracker** | Create/modify/delete/rename in watched project roots | Phase 2 — optional |

### 3.1 Collector Notes

- **Activity & Focus Watcher** is the foundation of idle detection. Without it, reading code or thinking without editing is invisible, and breaks cannot be distinguished from work.
  - Windows: `GetLastInputInfo`, `GetForegroundWindow`.
  - macOS: `CGEventSourceSecondsSinceLastEventType`, `NSWorkspace` (requires Accessibility permission for window titles).
  - Linux: X11 (`XScreenSaver`, `_NET_ACTIVE_WINDOW`); Wayland support is compositor-specific and limited — document as best-effort.
- **VS Code Extension** is the richest project signal; it knows the workspace exactly.
- **Git Tracker** can **backfill** past weeks from commit history, giving value on day one.
- **Terminal Tracker** requires per-shell hooks (Bash `PROMPT_COMMAND`/`DEBUG` trap, Zsh `preexec`/`precmd`, PowerShell `prompt` / PSReadLine, Fish events). Commands from the VS Code integrated terminal must be **deduplicated** against the VS Code extension.
- **File System Tracker** is mostly redundant with the VS Code extension; it is useful for editors without an extension (JetBrains, Vim, etc.). Must ignore noise: `.git/`, `node_modules/`, `target/`, `dist/`, `build/`, `.venv/`, and `.gitignore`d paths. Watch only configured project roots to respect OS watcher limits (e.g., Linux `inotify` limits).

### 3.2 Collector Reliability

- Every collector **buffers events locally** (bounded queue on disk) when the daemon is unavailable and retries with backoff.
- Ingestion is **idempotent**: events are deduplicated by Event ID.
- Collectors send a periodic **heartbeat** so the daemon can detect a dead collector and mark gaps in coverage instead of treating them as idle time.

## 4. Central Event Collection

All collectors send events to the daemon through a unified, authenticated local ingestion API.

### 4.1 Event Schema

| Field | Description |
|---|---|
| `event_id` | UUIDv7 (time-sortable), generated by the collector |
| `schema_version` | Event schema version |
| `timestamp_start` | UTC start time |
| `timestamp_end` | UTC end time (nullable for point-in-time events) |
| `device_id` | Stable per-machine identifier |
| `source` | Collector name and version (e.g., `vscode@0.3.1`) |
| `event_type` | e.g., `file.edit`, `git.commit`, `terminal.command`, `browser.tab_focus`, `presence.idle_start` |
| `project_hint` | Project ID if the collector knows it (nullable) |
| `repository` | Repository root, if available |
| `working_directory` | Working directory, if available |
| `metadata` | Event-specific JSON payload (already redacted) |

Raw events are **immutable**. Project assignment is stored separately (see §8), never by mutating the event.

### 4.2 Ingestion Security

- Local HTTP API bound to `127.0.0.1` only.
- Each collector is registered and receives a **per-collector token**; requests without a valid token are rejected.
- Strict CORS: arbitrary websites must not be able to post events to the daemon.
- Browser extension preferably uses **Native Messaging** instead of HTTP.

## 5. Project Correlation Engine

Associates each event with a project, producing an **assignment** with a confidence score and a source.

### 5.1 Resolution Order

1. **User override** — manual assignment (confidence 1.0).
2. **Explicit mapping rules** — path prefix, repository remote URL, domain/URL pattern, window title pattern (confidence 0.95).
3. **Structural signals** — repository root, VS Code workspace, terminal working directory (confidence 0.9).
4. **Temporal context** — an unassigned event surrounded by events of one project within a short window inherits that project (confidence 0.6–0.8).
5. **AI classification** — only for events still ambiguous after steps 1–4 (confidence reported by the classifier, capped at 0.8).
6. **Unassigned** — shown in the dashboard for review.

### 5.2 Learning From Corrections

When the user corrects an assignment, the system proposes a new mapping rule (e.g., "Always assign `github.com/acme/*` to project Acme?"). Accepted rules are deterministic and reduce future AI usage.

### 5.3 Assignment Record

Each assignment stores: `event_id`, `project_id`, `confidence`, `method` (`user` | `rule` | `structural` | `temporal` | `ai`), `rule_id` or `model_id`, and `created_at`.

AI classifications are **cached**, so re-running analysis produces the same result unless the user explicitly requests reclassification.

## 6. Work Time Calculation Engine

Reconstructs work sessions deterministically from events. The same events + same assignments + same algorithm version **always** produce the same sessions.

### 6.1 Default Rules (configurable)

**Event credit** — how much time each event type contributes around itself:

| Event | Default credit |
|---|---|
| Editor edit / save | 2 min |
| Editor focus on project file | Actual focus duration while not idle |
| Terminal command | Actual run duration (capped at user-active time; see §6.3) |
| Git commit / checkout | 2 min |
| Browser tab focus (assigned to a project) | Actual focus duration while not idle |
| File system change (no editor signal) | 1 min |

**Session building:**

- Events of the same project separated by a gap **≤ 10 minutes** are merged into one session; the gap is counted as work.
- A gap **> 10 minutes** ends the session.
- Any period where the Activity Watcher reports **idle ≥ 5 minutes** is excluded from sessions, regardless of other events (except see §6.3).
- Sessions shorter than **1 minute** are dropped as noise.

### 6.2 Overlap Resolution

Only one project can receive time at any moment. When signals overlap, priority is:

1. Focused window's project (from the Activity & Focus Watcher)
2. VS Code active workspace
3. Terminal (foreground command)
4. Browser
5. File system

### 6.3 Background and Waiting Time

- A long-running command (build, test suite, deploy) where the user is **idle** is recorded as **waiting time**, not active work.
- Waiting time is shown separately and can be included in billable time per project setting (e.g., "count build waiting time as billable").
- Background file changes with no presence signal (e.g., a build writing to `dist/`) are ignored.

### 6.4 Explainability

- Every session links to its **supporting events** (`session_evidence`).
- Every session stores the **algorithm version** and the **configuration hash** used to produce it.
- The dashboard can show "why this session exists" — the events and rules that produced it.

### 6.5 Recalculation

Sessions are derived data. Changing mappings, assignments, rules, or the algorithm triggers recalculation of affected days. Manual time adjustments (§8 `time_adjustments`) are reapplied on top of recalculated sessions.

## 7. AI Integration

AI is an assistant, never the source of truth for durations.

### 7.1 Responsibilities

- Classify events left ambiguous after rule-based correlation.
- Associate browser research (docs, issues, Stack Overflow) with the project being worked on.
- Suggest new mapping rules from correction patterns.
- Summarize daily and weekly activity per project ("Implemented auth flow, fixed 3 CI failures").
- Draft report descriptions for invoices/timesheets.

### 7.2 Constraints

- **Grounded:** summaries reference only collected events; each statement must be traceable to evidence.
- **Deterministic results:** classifications are cached; reports do not change on regeneration unless data changed.
- **Minimal data:** only redacted, minimal fields are sent (e.g., URL domain + title, not full URLs; file path relative to repo, not contents).
- **Provider choice:** supports a **local model** (e.g., via Ollama) and an optional **cloud provider**. Local-only mode must be available and fully functional except for AI features.
- **Budget:** configurable daily limit on AI calls/tokens; batch classification to reduce cost.
- AI is fully optional — the system works with rules only.

## 8. Data Storage

Local-first SQLite database, owned by the daemon.

| Entity | Purpose |
|---|---|
| `activity_events` | Immutable raw events |
| `collectors` | Registered collectors, tokens (hashed), last heartbeat |
| `projects` | Tracked projects, hourly rate, billable settings |
| `project_mappings` | Rules: paths, remotes, domains, title patterns → project |
| `event_assignments` | Event → project with confidence, method, source |
| `ai_classifications` | Cached AI results (input hash → output) |
| `work_sessions` | Derived intervals with algorithm version + config hash |
| `session_evidence` | Session → supporting events |
| `time_adjustments` | Manual corrections (add, trim, split, reassign) |
| `coverage_gaps` | Periods where a collector was offline |

- Daily/weekly report aggregates are **computed views or caches**, not a source of truth.
- Database encrypted at rest (e.g., SQLCipher), with the key stored in the OS keychain.
- Configurable **retention**: e.g., raw events kept 1 year, then compacted to sessions only.
- Schema migrations versioned and applied by the daemon.

## 9. Multi-Device Support (Phase 4)

- Each device has its own local database; `device_id` is present from day one.
- Sync is optional, end-to-end encrypted, and user-controlled (self-hosted folder, or peer-to-peer).
- Timestamps are stored in UTC; clock skew between devices is tolerated by merging sessions per device first, then combining.
- Simultaneous activity on two devices for different projects is surfaced for user review.

## 10. Dashboard

Delivered as a Tauri desktop app (React + TypeScript), plus a system tray icon.

### 10.1 MVP Dashboard (Phase 1)

- Today/this week: time per project.
- Timeline view of sessions per project.
- Session detail with supporting evidence ("why this session").
- **Manual corrections:** reassign a session, trim/extend, split, delete.
- Unassigned activity inbox.
- Project and mapping management.
- Export to CSV.

### 10.2 Later Dashboard Features

- AI daily/weekly summaries.
- Billable vs non-billable time, hourly rates, cost per project.
- Estimated vs actual cost.
- Excel export; integrations (Harvest, Toggl, Jira, Clockify).
- Rounding rules for invoicing (e.g., round to 6/15 minutes).
- Collector health and coverage gaps.
- Privacy view: inspect and delete any stored data.

## 11. Privacy and Security

Local-first and privacy-by-default.

### 11.1 Collection

- Never collect file contents, keystroke content, clipboard, or screenshots.
- **Terminal:** store the **command name only** by default (e.g., `npm`, `cargo`, `git push`); full arguments are opt-in per project, and are still passed through redaction.
- **Browser:** strip query strings and fragments by default; incognito/private windows are never tracked.
- Redaction filters for known secret patterns (API keys, tokens, `Authorization` headers, `KEY=value` assignments, connection strings). Redaction happens **in the collector, before** data leaves it.
- Exclusion lists: directories, applications, domains, and time windows (e.g., "never track after 19:00").
- Pause tracking with one click (tray icon), with a visible tracking indicator.

### 11.2 Storage and Access

- Ingestion API bound to localhost with per-collector tokens.
- Database encrypted at rest.
- User can inspect, export, and delete any data (per event, per day, per project, or everything).

### 11.3 AI Data Flow

- The dashboard states clearly which fields are sent to AI and to which provider.
- Cloud AI is opt-in; local AI or no AI are supported.
- AI requests and responses are logged locally for auditability.

## 12. Technology Stack

| Component | Technology |
|---|---|
| Background daemon | Rust |
| Activity & Focus Watcher | Rust (platform-specific modules) |
| Calculation & correlation engine | Rust (inside the daemon) |
| AI integration | Rust HTTP client to local (Ollama) or cloud LLM APIs |
| Desktop app & tray | Tauri |
| Dashboard | React + TypeScript |
| Database | SQLite (+ SQLCipher) |
| Event ingestion | Local HTTP API (localhost, token auth) |
| VS Code integration | TypeScript, VS Code Extension API |
| Browser integration | WebExtensions API + Native Messaging |
| Terminal integration | Bash, Zsh, Fish, PowerShell hooks |

Python is intentionally removed from the core: bundling a Python runtime across three operating systems adds significant packaging complexity, and LLM APIs can be called directly from Rust. Python may be reintroduced later only if local custom ML models are needed.

Supported platforms: Windows, macOS, Linux (X11 fully; Wayland best-effort).

## 13. Development Roadmap

### Phase 0 — Validation (1–2 weeks)
- Evaluate ActivityWatch watchers for reuse or interoperability.
- Prototype the Activity & Focus Watcher on all three OSes.
- Keep a manual time log for 2 weeks to serve as the accuracy baseline.

### Phase 1 — MVP
- Rust daemon with authenticated local ingestion API and SQLite storage.
- Activity & Focus Watcher (idle + focused window).
- VS Code extension.
- Git tracker with historical backfill.
- Rule-based project correlation (paths, repositories, mappings).
- Deterministic time calculation engine (§6 default rules).
- Tauri dashboard: daily/weekly view, timeline, evidence, **manual corrections**, unassigned inbox.
- CSV export.
- Pause tracking and exclusion lists.

### Phase 2 — More Collectors and AI
- Terminal tracker (Bash, Zsh, PowerShell, Fish) with VS Code terminal deduplication.
- Browser extension with Native Messaging.
- File system tracker for editors without an extension.
- AI-assisted classification for ambiguous events (local and cloud).
- Rule suggestions from corrections.
- AI daily/weekly summaries.
- Collector health and coverage gaps.

### Phase 3 — Billing and Reporting
- Project hourly rates, billable/non-billable settings.
- Waiting-time billing policy.
- Rounding rules.
- Estimated vs actual cost.
- Excel export and integrations (Harvest, Toggl, Jira, Clockify).
- Advanced analytics.

### Phase 4 — Multi-Device
- Encrypted, user-controlled sync.
- Cross-device session merging and conflict review.

## 14. Risks and Open Questions

| Risk / Question | Mitigation / Decision needed |
|---|---|
| Under-counting "thinking" time with no events | Presence + focus signals; configurable gap threshold; validation against manual baseline |
| Wayland restricts focus/window access | Best-effort support; document limitations |
| macOS permissions (Accessibility, Screen Recording for titles) | Clear onboarding flow explaining each permission |
| Secret leakage via terminal/URLs | Command-name-only default, in-collector redaction, audit test suite |
| AI non-determinism | Cache classifications; AI never sets durations |
| Default thresholds may not fit every user | Make all rules configurable; tune using Phase 0 baseline |
| Scope creep | Strict phase gates tied to success metrics |

## Final Goal

Create a reliable, private, cross-platform development activity tracker that automatically reconstructs project work sessions from real evidence, calculates time spent on each project with explainable and correctable results, and produces accurate time-cost reports — without manual timers or constant project switching management.
