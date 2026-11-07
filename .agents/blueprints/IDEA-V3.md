# Cross-Platform AI Work Time Tracker — Version 3

> **Status:** Draft blueprint · **Supersedes:** IDEA.md, IDEA-V2.md
>
> **Structure:** Part I — Product (why & what) · Part II — System Design (how) · Part III — Delivery (when) · Appendices

---

# Part I — Product

## 1. Summary

A local-first, cross-platform background daemon that collects development activity across applications, reconstructs work sessions from that evidence, and reports the actual time spent on each project — without manual timers.

Time is calculated by a **deterministic, explainable engine**. AI is an optional assistant that classifies ambiguous activity and writes summaries; it never decides durations.

**Core loop:**

**Collect → Centralize → Correlate → Calculate → Correct → Report**

1. **Collect** activity events from collectors (editor, Git, presence, terminal, browser, file system).
2. **Centralize** them in an immutable local event store.
3. **Correlate** each event with a project — rules first, AI only for leftovers.
4. **Calculate** work sessions deterministically from evidence.
5. **Correct** — the user reviews and adjusts; corrections become rules.
6. **Report** time and cost per project.

## 2. Problem and Target Users

### 2.1 Problem

Developers who switch between several projects per day cannot reliably answer *"how much time did I spend on project X?"*

- Manual timers are forgotten or left running.
- App-usage trackers measure *which application* was open, not *which project* was being worked on.
- Reconstructing time from memory, commits, and browser history at the end of the week is slow and inaccurate.

### 2.2 Target Users

| Persona | Situation | Main need |
|---|---|---|
| **Freelance developer** (primary) | 3–6 client projects in parallel, bills hourly | Accurate, defensible billable hours per client |
| **Agency / consultancy developer** | Logs time to client budgets and tickets | Low-effort timesheets, export to Jira/Harvest |
| **Indie / side-project developer** | Splits time between job, side projects, open source | Insight into where time actually goes |

### 2.3 Current Workarounds

- Start/stop timers (Toggl, Clockify) — rely on discipline.
- Editor-only trackers (WakaTime) — miss browser, terminal, and reading time.
- End-of-week reconstruction from Git log and calendar — inaccurate and tedious.

## 3. Goals, Non-Goals, and Success Metrics

### 3.1 Goals

- Automatically attribute working time to the correct project across applications.
- Make every calculated minute **explainable** (traceable to evidence) and **correctable**.
- Keep all data local and under the user's control.
- Produce billing-ready reports.

### 3.2 Non-Goals

- Not an employee surveillance or monitoring tool.
- No screenshots, screen recording, clipboard capture, or keystroke content logging.
- No collection of full file contents.
- No cloud dependency for core functionality.
- AI never invents or decides durations.

### 3.3 Success Metrics

| Metric | Target |
|---|---|
| Accuracy vs. manual log (2-week trial) | Within **±10%** per project per week |
| Automatic assignment rate (after initial mappings) | **≥ 90%** of active time |
| Daily review effort | **< 2 minutes** per day |
| Daemon overhead | **< 1% CPU** average, **< 100 MB RAM** |
| Secret leakage (redaction audit) | **Zero** secrets in stored data |

## 4. Positioning

| Tool | Strength | Gap this project addresses |
|---|---|---|
| **WakaTime** | Mature editor plugins | Editor-centric, cloud-first; weak correlation of browser/terminal work to projects |
| **ActivityWatch** | Open-source, local-first, window/AFK/browser watchers | Tracks apps and windows, not projects; no billing-oriented reports |
| **RescueTime / Timing** | Automatic, polished | Productivity categories, not per-project evidence-backed time cost |
| **Toggl / Clockify / Harvest** | Billing and invoicing | Manual timers |

**Differentiator:** cross-application *project correlation* + explainable session reconstruction + billing-ready output, fully local-first.

> ActivityWatch's watchers are open source. Reusing or interoperating with them is evaluated in Phase 0 before building equivalent collectors.

## 5. User Experience

### 5.1 Onboarding (first run, ~5 minutes)

1. Install the desktop app; the daemon starts at login and shows a tray icon.
2. Grant OS permissions, each with a plain-language explanation (e.g., macOS Accessibility for window titles).
3. Choose project roots (e.g., `~/code`). The app detects repositories and proposes one project per repository; the user merges, renames, or ignores.
4. Install the VS Code extension (one click, auto-registers with the daemon).
5. Optional: **backfill** the last 4 weeks from Git history to see an initial (approximate) report immediately.
6. Choose AI mode: **Off** (default) · **Local** · **Cloud**.

### 5.2 A Normal Day

- Tracking runs silently. The tray icon shows the current project and today's total.
- One click on the tray: **Pause tracking** (15 min / 1 h / until resumed) or **Mark as private**.
- No prompts or interruptions during work.

### 5.3 End-of-Day Review (< 2 minutes)

1. Open the dashboard → **Today** timeline: one lane per project.
2. **Unassigned inbox** lists activity that could not be attributed confidently. The user assigns it in bulk.
3. After a correction, the app proposes a rule: *"Always assign `github.com/acme/*` to Acme?"* → Accept / Not now.
4. Any session can be opened to see **why it exists** (supporting events and rules) and then trimmed, split, reassigned, or deleted.

### 5.4 Weekly Report / Invoice

1. **Reports** → choose a week and project(s).
2. Review totals: active time, waiting time, billable time, cost.
3. Optional: generate an AI summary of work done ("Implemented auth flow; fixed 3 CI failures"), grounded in evidence.
4. Export CSV/Excel or push to an integration (Phase 3).

### 5.5 Privacy Controls (always available)

- See exactly what was collected, per event.
- Delete any event, day, project, or all data.
- Manage exclusions (directories, apps, domains, time windows).
- See which data, if any, was sent to an AI provider.

---

# Part II — System Design

## 6. Architecture Overview

```
 ┌───────────────────────────── Collectors ─────────────────────────────┐
 │ Activity&Focus │ VS Code Ext │ Git │ Terminal Hooks │ Browser Ext │ FS │
 └───────┬─────────────┬──────────┬────────┬───────────────┬─────────┬──┘
         │  redact at source · local buffer · heartbeat · token auth   │
         ▼                                                              ▼
 ┌──────────────────────────── Rust Daemon ─────────────────────────────┐
 │  Ingestion API (127.0.0.1, per-collector tokens, Native Messaging)   │
 │        │                                                             │
 │        ▼                                                             │
 │  Event Store (immutable activity_events)                             │
 │        │                                                             │
 │        ▼                                                             │
 │  Correlation Engine ──(ambiguous only)──► AI Layer (local / cloud)   │
 │        │  event_assignments                     │ cached results     │
 │        ▼                                        ▼                    │
 │  Time Calculation Engine ──► work_sessions + session_evidence        │
 │        │                                                             │
 │        ▼                                                             │
 │  Adjustments applied ──► Reports / Aggregates                        │
 └───────────────────────────────┬──────────────────────────────────────┘
                                 │ local IPC
                                 ▼
                ┌──────────── Tauri Desktop App ────────────┐
                │ Tray · Dashboard (React + TS) · Settings  │
                └───────────────────────────────────────────┘
```

**Principles:**

- **Raw is immutable, everything else is derived.** Assignments, sessions, and reports can always be recomputed.
- **Deterministic core.** Same inputs + same rules + same algorithm version → same output.
- **Privacy at the edge.** Redaction happens inside collectors, before data reaches the daemon.
- **AI at the margin.** AI is called only for what rules cannot resolve, and its results are cached.

## 7. Glossary / Domain Model

| Term | Definition |
|---|---|
| **Event** | An immutable record of one observed activity (point-in-time or interval) from one collector. |
| **Presence** | Whether the user is actively using the computer, derived from keyboard/mouse input recency. |
| **Idle** | A period with no user input for at least the idle threshold. |
| **Project** | A unit of work time is reported against (usually a client or product), possibly spanning several repositories. |
| **Mapping** | A deterministic rule linking a signal (path, remote, domain, title pattern) to a project. |
| **Assignment** | The link between one event and one project, with a confidence score and method. |
| **Session** | A continuous derived interval of work on one project. |
| **Evidence** | The set of events that justify a session. |
| **Active time** | Session time during which the user is present. |
| **Waiting time** | Time a project's background task runs while the user is idle (e.g., a long build). |
| **Adjustment** | A manual correction to sessions (add, trim, split, reassign, delete), reapplied after recalculation. |
| **Coverage gap** | A period when a collector was not running, so absence of events is not evidence of idleness. |
| **Algorithm version** | Version identifier of the calculation rules that produced a session. |

## 8. Collectors

### 8.1 Collector Contract (applies to every collector)

- **Redact at source** before sending (see §14).
- **Buffer locally** in a bounded on-disk queue when the daemon is unavailable; retry with backoff.
- **Generate UUIDv7 event IDs** so ingestion is idempotent.
- **Send heartbeats** every 60 s so the daemon can record coverage gaps.
- **Authenticate** with a per-collector token issued at registration.
- **Respect exclusions and pause state** pushed by the daemon.
- **Stay lightweight**: no noticeable impact on editor, shell, or browser performance.

### 8.2 Collector Specifications

| # | Collector | Signals | Phase |
|---|---|---|---|
| 1 | **Activity & Focus Watcher** | Input idle state (no key content), focused application, window title | 1 — required |
| 2 | **VS Code Extension** | Workspace, active file, edits, saves, debug sessions, integrated terminal | 1 |
| 3 | **Git Tracker** | Repos, branches, commits, checkouts, merges; historical backfill | 1 |
| 4 | **Terminal Tracker** | Command name, cwd, start/end, exit code | 2 |
| 5 | **Browser Extension** | Redacted URL, page title, active tab focus duration | 2 |
| 6 | **File System Tracker** | Create/modify/delete/rename in project roots | 2 — optional |

**1. Activity & Focus Watcher** — the foundation of presence and idle detection. Without it, reading or thinking without editing is invisible and breaks are indistinguishable from work. Runs inside the daemon process.

**2. VS Code Extension** — the richest project signal; knows the workspace exactly. Reports focus intervals, edit bursts (aggregated, e.g., per 30 s), saves, and debug sessions. Tags integrated-terminal commands so the terminal tracker can deduplicate them.

**3. Git Tracker** — watches registered repositories for commits, checkouts, and merges; resolves remote URL for mapping. **Backfill** imports commit history to produce an approximate historical report on day one (clearly marked as "estimated from commits").

**4. Terminal Tracker** — per-shell hooks: Bash (`PROMPT_COMMAND` + `DEBUG` trap), Zsh (`preexec`/`precmd`), Fish (event handlers), PowerShell (`prompt` / PSReadLine). Stores command **name only** by default. Deduplicates against VS Code integrated terminal events.

**5. Browser Extension** — WebExtensions for Chromium and Firefox; communicates via **Native Messaging**. Tracks focused tab only; strips query strings and fragments; never tracks private/incognito windows.

**6. File System Tracker** — for editors without an extension (JetBrains, Vim, etc.). Watches configured project roots only; ignores `.git/`, `node_modules/`, `target/`, `dist/`, `build/`, `.venv/`, and `.gitignore`d paths. Mostly redundant when the VS Code extension is active.

## 9. Event Ingestion and Schema

### 9.1 Ingestion

- Local HTTP API bound to `127.0.0.1` only; browser extension uses Native Messaging.
- Per-collector bearer token (stored hashed in the daemon).
- Strict CORS: websites cannot post events.
- Idempotent writes keyed by `event_id`; duplicates are discarded.
- Schema validation per `schema_version`; invalid events are rejected and counted in collector health.

### 9.2 Event Schema

| Field | Type | Description |
|---|---|---|
| `event_id` | UUIDv7 | Time-sortable, generated by the collector |
| `schema_version` | int | Event schema version |
| `timestamp_start` | UTC datetime | Start time |
| `timestamp_end` | UTC datetime, nullable | End time for interval events |
| `device_id` | string | Stable per-machine identifier |
| `source` | string | Collector name and version, e.g. `vscode@0.3.1` |
| `event_type` | string | See Appendix B |
| `project_hint` | string, nullable | Project ID if the collector knows it |
| `repository` | string, nullable | Repository root |
| `working_directory` | string, nullable | Working directory |
| `metadata` | JSON | Event-specific payload, already redacted |

Raw events are never mutated. Project assignment is stored separately.

## 10. Project Correlation

### 10.1 Resolution Order

| Step | Method | Confidence |
|---|---|---|
| 1 | **User override** — manual assignment | 1.0 |
| 2 | **Mapping rule** — path prefix, remote URL, domain/URL pattern, window title pattern | 0.95 |
| 3 | **Structural signal** — repo root, VS Code workspace, terminal cwd | 0.9 |
| 4 | **Temporal context** — unassigned event surrounded by one project's events within the context window | 0.6–0.8 |
| 5 | **AI classification** — only if still ambiguous | ≤ 0.8 |
| 6 | **Unassigned** — goes to the review inbox | — |

Events below the **review threshold** (default 0.7) are surfaced in the inbox even if assigned.

### 10.2 Assignment Record

`event_id`, `project_id`, `confidence`, `method` (`user` | `rule` | `structural` | `temporal` | `ai`), `rule_id` or `model_id`, `created_at`.

### 10.3 Learning From Corrections

When the user corrects assignments with a shared pattern, the system proposes a mapping rule. Accepted rules are deterministic and reduce future AI calls. Rejected proposals are remembered and not repeated.

## 11. Time Calculation

### 11.1 Inputs and Outputs

- **Inputs:** events, assignments, presence intervals, coverage gaps, configuration (Appendix A).
- **Outputs:** `work_sessions` (active and waiting), `session_evidence`.
- Each session records `algorithm_version` and `config_hash`.

### 11.2 Algorithm

1. **Build presence timeline** from the Activity & Focus Watcher: present vs. idle (idle ≥ idle threshold).
2. **Credit events** with time around them per event type (Appendix A, *event credit*). Interval events (focus, command run) use their actual duration.
3. **Clip to presence:** remove credited time that falls inside idle periods — except background tasks, which become **waiting time** candidates.
4. **Resolve overlaps:** at any instant only one project receives active time, chosen by signal priority:
   1. Focused window's project
   2. VS Code active workspace
   3. Terminal foreground command
   4. Browser
   5. File system
5. **Merge** same-project intervals separated by gaps ≤ merge gap; the gap counts as active time if the user was present.
6. **Drop** sessions shorter than the minimum session length.
7. **Mark coverage gaps:** if presence data is missing (watcher offline), sessions in that range are flagged *low confidence* instead of silently counted or dropped.
8. **Apply adjustments** from `time_adjustments` on top of derived sessions.

### 11.3 Waiting Time

- A long-running command (build, tests, deploy) while the user is idle is **waiting time**, not active time.
- Waiting time is reported separately; each project decides whether it is billable.
- Background file changes with no presence signal (e.g., build output) are ignored.

### 11.4 Explainability and Recalculation

- Every session links to its evidence; the dashboard shows "why this session exists."
- Changing mappings, assignments, configuration, or algorithm version triggers recalculation of affected days only.
- Adjustments are anchored to time ranges and projects, so they survive recalculation.

## 12. AI Layer

### 12.1 Uses

| Use | Phase |
|---|---|
| Classify events left ambiguous after rules | 2 |
| Associate browser research (docs, issues, Q&A) with the active project | 2 |
| Suggest mapping rules from correction patterns | 2 |
| Daily and weekly activity summaries per project | 2 |
| Draft invoice/timesheet line descriptions | 3 |

### 12.2 Rules

- **Never decides durations.** Only classifies and summarizes.
- **Grounded:** every summary statement must reference supporting events.
- **Cached:** results stored by input hash; regeneration is stable unless data changed.
- **Minimal input:** only redacted, minimal fields (domain + title, repo-relative paths, command names). Never file contents.
- **Batched** to reduce cost and latency.
- **Budgeted:** configurable daily token/call limit; when exceeded, events stay unassigned rather than failing.
- **Optional:** the full product works with AI off.

### 12.3 Providers

| Mode | Description |
|---|---|
| **Off** (default) | Rules only |
| **Local** | Local model server (e.g., Ollama); no data leaves the machine |
| **Cloud** | User-supplied API key; explicit opt-in; every request logged locally |

## 13. Data Model and Storage

Local SQLite database owned by the daemon, encrypted at rest (SQLCipher, key in OS keychain).

| Entity | Kind | Purpose |
|---|---|---|
| `activity_events` | Raw | Immutable events |
| `collectors` | Config | Registered collectors, hashed tokens, last heartbeat, health |
| `projects` | Config | Projects, hourly rate, billable and waiting-time policy |
| `project_mappings` | Config | Rules: paths, remotes, domains, title patterns → project |
| `rule_proposals` | Config | Suggested rules and their accepted/rejected state |
| `exclusions` | Config | Excluded directories, apps, domains, time windows |
| `event_assignments` | Derived | Event → project, confidence, method |
| `ai_classifications` | Cache | AI results keyed by input hash |
| `ai_request_log` | Audit | What was sent to which provider, when |
| `work_sessions` | Derived | Sessions with type (active/waiting), algorithm version, config hash |
| `session_evidence` | Derived | Session → supporting events |
| `time_adjustments` | User data | Manual corrections |
| `coverage_gaps` | Derived | Periods when collectors were offline |

- Reports and daily/weekly totals are **computed views or caches**, never a source of truth.
- **Retention** (configurable): raw events kept 12 months, then compacted to sessions + evidence summaries.
- Schema migrations are versioned and applied by the daemon at startup.

**Multi-device (Phase 4):** each device keeps its own database; `device_id` exists from day one. Optional end-to-end encrypted sync (self-hosted folder or peer-to-peer). Sessions are merged per device first, then combined; simultaneous activity on two devices for different projects is surfaced for review. All timestamps are UTC.

## 14. Privacy and Security

### 14.1 Threat Model

| Threat | Mitigation |
|---|---|
| Secrets captured in terminal commands (`export API_KEY=…`, `curl -H "Authorization: …"`) | Store **command name only** by default; full arguments opt-in per project; secret-pattern redaction in the collector |
| Tokens or personal data in URLs | Strip query strings and fragments; domain + path only; per-domain exclusions |
| Personal browsing captured | Track only focused tab; never private windows; domain allow/deny lists; time-window exclusions |
| Malicious local process or website posting fake events | Localhost binding, per-collector tokens, strict CORS, Native Messaging for browser |
| Database stolen from disk | Encryption at rest; key in OS keychain |
| Data sent to cloud AI | AI off by default; explicit opt-in; minimal redacted fields; local request log |
| User unaware of what is collected | Inspectable data view; visible tracking indicator; one-click pause |
| Over-collection drift over time | Redaction audit test suite in CI; collector payloads validated against schema |

### 14.2 Principles

- Collect the minimum needed to attribute time.
- Redact at the source.
- The user can inspect, export, and delete everything.
- Nothing leaves the machine unless the user opts in.

## 15. Platform Support Matrix

| Collector | Windows | macOS | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| Idle detection | ✅ `GetLastInputInfo` | ✅ `CGEventSource` | ✅ XScreenSaver | ⚠️ Compositor-specific (e.g., `ext-idle-notify`) |
| Focused window / title | ✅ `GetForegroundWindow` | ⚠️ Requires Accessibility permission | ✅ `_NET_ACTIVE_WINDOW` | ⚠️ Limited; GNOME/KDE extensions |
| VS Code extension | ✅ | ✅ | ✅ | ✅ |
| Git tracker | ✅ | ✅ | ✅ | ✅ |
| Terminal hooks | ✅ PowerShell, Git Bash | ✅ Zsh, Bash, Fish | ✅ Bash, Zsh, Fish | ✅ Bash, Zsh, Fish |
| Browser extension | ✅ Chromium, Firefox | ✅ Chromium, Firefox | ✅ Chromium, Firefox | ✅ Chromium, Firefox |
| File system watcher | ✅ `ReadDirectoryChangesW` | ✅ FSEvents | ✅ inotify (watch limits) | ✅ inotify (watch limits) |

✅ supported · ⚠️ best-effort / requires permission or extension

## 16. Technology Stack and Key Decisions

### 16.1 Stack

| Component | Technology |
|---|---|
| Daemon, ingestion, correlation, calculation | Rust |
| Activity & Focus Watcher | Rust, platform-specific modules |
| AI client | Rust HTTP client → local (Ollama) or cloud LLM APIs |
| Desktop app and tray | Tauri |
| Dashboard | React + TypeScript |
| Database | SQLite + SQLCipher |
| Ingestion transport | Local HTTP (localhost, token auth); Native Messaging for browser |
| VS Code integration | TypeScript, VS Code Extension API |
| Browser integration | WebExtensions API |
| Terminal integration | Bash, Zsh, Fish, PowerShell hooks |

### 16.2 Decision Log

| # | Decision | Rationale |
|---|---|---|
| D1 | Deterministic calculation; AI never sets durations | Billing requires reproducible, defensible numbers |
| D2 | Raw events immutable; all else derived | Enables recalculation when rules or algorithms improve |
| D3 | No Python in the core | Bundling Python across 3 OSes is costly; LLM APIs callable from Rust. Revisit only for custom local ML |
| D4 | Presence watcher is mandatory in MVP | Idle detection and reading time are impossible without it |
| D5 | Redaction inside collectors | Sensitive data never reaches storage |
| D6 | AI off by default | Privacy-first; product must stand on rules alone |
| D7 | Native Messaging for browser | Avoids exposing a localhost HTTP surface to web pages |
| D8 | SQLite, single-writer daemon | Simple, embedded, sufficient for single-user local data |

---

# Part III — Delivery

## 17. Roadmap

### Phase 0 — Validation (1–2 weeks)

- Evaluate ActivityWatch watchers for reuse or interoperability.
- Prototype the Activity & Focus Watcher on Windows, macOS, and Linux.
- Keep a manual time log for 2 weeks as the accuracy baseline.

**Exit criteria:** build-vs-reuse decision recorded; idle + focus prototype working on all three OSes; baseline log collected.

### Phase 1 — MVP

- Rust daemon: authenticated ingestion, SQLite storage, collector contract.
- Activity & Focus Watcher.
- VS Code extension.
- Git tracker with backfill.
- Rule-based correlation (steps 1–4, 6 of §10.1).
- Time calculation engine (§11) with Appendix A defaults.
- Tauri app: tray, Today/Week views, timeline, session evidence, **manual corrections**, unassigned inbox, project and mapping management.
- Pause tracking and exclusions.
- CSV export.

**Exit criteria:** within **±15%** of the manual baseline for one week; ≥ 80% automatic assignment; daemon within overhead targets; redaction audit passes.

### Phase 2 — Collectors and AI

- Terminal tracker (Bash, Zsh, Fish, PowerShell) with VS Code deduplication.
- Browser extension via Native Messaging.
- File system tracker.
- AI layer: classification, research association, rule suggestions, summaries (Local and Cloud modes).
- Collector health and coverage gap views.

**Exit criteria:** within **±10%** of baseline over two weeks; ≥ 90% automatic assignment; daily review < 2 minutes.

### Phase 3 — Billing and Reporting

- Hourly rates; billable/non-billable and waiting-time policy per project.
- Rounding rules (e.g., 6 / 15 minutes).
- Estimated vs actual cost.
- Excel export; integrations (Harvest, Toggl, Jira, Clockify).
- AI-drafted invoice line descriptions.
- Advanced analytics.

**Exit criteria:** a full monthly invoice can be produced from the app without manual spreadsheet work.

### Phase 4 — Multi-Device

- End-to-end encrypted, user-controlled sync.
- Cross-device session merging and conflict review.

**Exit criteria:** two devices produce a single consistent weekly report with no double-counted time.

## 18. Risks and Open Questions

### 18.1 Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Under-counting thinking/reading time | Low accuracy | Presence + focus signals; tunable merge gap; validated against baseline |
| Wayland limits focus/idle access | Reduced Linux accuracy | Best-effort support; document limitations; desktop-specific extensions |
| macOS permission friction | Onboarding drop-off | Clear permission explanations; app degrades gracefully without titles |
| Secret leakage | Trust loss | Command-name-only default; in-collector redaction; audit suite in CI |
| AI non-determinism | Unstable reports | Caching; AI never sets durations |
| Default thresholds fit some users poorly | Accuracy varies | All values configurable; tuning guided by Phase 0 baseline |
| Scope creep | Delayed MVP | Phase exit criteria are hard gates |

### 18.2 Open Questions

1. Build our own collectors or extend ActivityWatch? (Decided in Phase 0.)
2. How should time on unrelated activities (email, chat, meetings) be handled — ignored, "overhead" project, or calendar integration?
3. Should projects support sub-units (tasks, tickets, branches) in reporting?
4. Which editors beyond VS Code get native extensions, and when (JetBrains, Neovim)?
5. Licensing and distribution model (open source, paid, freemium)?

---

# Appendices

## Appendix A — Default Configuration Values

All values are configurable and will be tuned using the Phase 0 baseline.

| Setting | Default | Used in |
|---|---|---|
| Idle threshold | 5 min | §11.2 step 1 |
| Merge gap | 10 min | §11.2 step 5 |
| Minimum session length | 1 min | §11.2 step 6 |
| Temporal context window | 5 min | §10.1 step 4 |
| Review threshold (confidence) | 0.7 | §10.1 |
| Collector heartbeat interval | 60 s | §8.1 |
| VS Code edit aggregation window | 30 s | §8.2 |
| Raw event retention | 12 months | §13 |
| AI daily budget | Off (0) | §12.2 |

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

**Signal priority (overlap resolution):** focused window → VS Code workspace → terminal → browser → file system.

## Appendix B — Event Type Catalog

| Event type | Collector | Kind | Key metadata |
|---|---|---|---|
| `presence.active` | Activity | Interval | — |
| `presence.idle` | Activity | Interval | — |
| `window.focus` | Activity | Interval | `app`, `title` (redacted) |
| `editor.workspace_open` | VS Code | Point | `workspace_path` |
| `editor.file_focus` | VS Code | Interval | `file_path` (repo-relative), `language` |
| `editor.edit_burst` | VS Code | Interval | `file_path`, `edit_count` |
| `editor.save` | VS Code | Point | `file_path` |
| `editor.debug_session` | VS Code | Interval | `configuration_name` |
| `git.commit` | Git | Point | `hash`, `branch`, `files_changed` |
| `git.checkout` | Git | Point | `from_branch`, `to_branch` |
| `git.merge` | Git | Point | `branch` |
| `git.backfill_commit` | Git | Point | `hash`, `branch`, `estimated: true` |
| `terminal.command` | Terminal | Interval | `command_name`, `exit_code`, `shell`, `from_vscode` |
| `browser.tab_focus` | Browser | Interval | `domain`, `path` (redacted), `title` |
| `fs.change` | File system | Point | `path` (repo-relative), `change_kind` |
| `collector.heartbeat` | All | Point | `collector_version`, `queue_depth` |

---

## Final Goal

A reliable, private, cross-platform development activity tracker that reconstructs project work sessions from real evidence, calculates time per project with explainable and correctable results, and produces accurate, billing-ready time-cost reports — without manual timers or constant project switching management.
