# CodeSecretary — Agent Instructions

**CodeSecretary** automatically tracks developer work time per project. A local agent collects redacted activity evidence (presence, editor, Git, terminal, browser) and uploads it to a cloud platform that deterministically reconstructs work sessions, lets users correct them, and (later) shares submitted time with teams — without surveillance.

**Stage:** early implementation. The Cargo workspace, the `agent/daemon` skeleton (AG-1), the warehouse foundations (WH-1–3: server, all migrations with RLS, Docker Compose) and CI exist. Shared, engine and overview are not started.

## Start Here

1. Read the project context: [.agents/context/README.md](.agents/context/README.md) → `config.md` → `structure.md` → files relevant to your task.
2. Find your task in the plan and progress files (below).
3. Verify against actual source code — context and plans describe intent; code is the truth once it exists.

## Project Documents

| Path | What |
|---|---|
| `.agents/blueprints/IDEA-V4.md` | Current product blueprint (what and why). V1–V3 are historical. |
| `.agents/plans/` | Implementation plans: `GENERAL.md` + one per service (`SHARED`, `AGENT`, `WAREHOUSE`, `ENGINE`, `OVERVIEW`). |
| `.agents/plans.progress/` | Checkbox progress per plan (`<NAME>.progress.md`). |
| `.agents/context/` | AI project memory — maintained via the `project-context` skill. |
| `.agents/skills/` | Project skills (`project-context`). |

## Services

| Folder | Service |
|---|---|
| `shared/` | Event schema, validation, redaction (Rust crate) |
| `agent/` | Local daemon + VS Code / browser / shell integrations |
| `warehouse/` | Device auth, ingest API, storage, **all DB migrations** |
| `engine/` | Correlation + time calculation (the only place time is computed) |
| `overview/` | User API + React dashboard |

## Non-Negotiable Rules

- **Only `engine/` computes time.** Overview reads results; everyone else enqueues jobs.
- **Deterministic engine.** AI never sets durations. Output changes require golden-test updates and an `ALGORITHM_VERSION` bump.
- **Redact in the agent** before data is buffered or sent. Never collect file contents, keystrokes, clipboard, or screenshots.
- **Never log event metadata** (titles, URLs, paths, commands).
- **Multi-tenant from day one.** Every tenant table has RLS in the same migration plus an isolation test. Tenant identity comes from the token/session, never the payload.
- **Private until submitted.** No feature may expose one user's raw activity to another user or admin.
- **Contracts** change only through `shared/` + migrations.

Full conventions: [.agents/context/conventions.md](.agents/context/conventions.md).

## Progress Tracking (required)

- Before starting a task, find its ID (e.g., `AG-8`, `EN-11`) in the matching plan and progress file.
- When you start a task, append `· 🚧 in progress` to its line in the progress file.
- When a task meets its acceptance criteria and the Definition of Done (`GENERAL.md` §9), change `- [ ]` to `- [x]` and append `· YYYY-MM-DD · short note`.
- If blocked, append `· ⛔ blocked: <reason>`.
- When all tasks of a milestone are done across services, tick it in `GENERAL.progress.md`, update its Status table, and add a Log row.
- Never tick a task whose acceptance criteria are not verified.
- **Update progress while working, not at the end.** The progress files must reflect the real state at every point, so work that stops halfway is still visible:
  - Mark `🚧` **before** writing the first line of code for a task, not after.
  - Tick each sub-item (e.g., a single migration) as soon as it is verified, not in one batch when the whole task is done.
  - Record deviations, decisions, and surprises in the progress file's **Notes** in the same step they happen.
  - When a decision or plan change rescopes a task, update that task's line and acceptance in the progress file right away, and fix any other task lines it makes stale.
  - Before ending a turn, re-read the progress lines you touched and make sure they match what was actually done and verified.

## Project Context (required)

`.agents/context/` is the project's AI memory: the smallest set of accurate, stable knowledge an agent needs to understand CodeSecretary and change it safely without re-exploring everything. It is maintained with the **`project-context` skill** ([.agents/skills/project-context/SKILL.md](.agents/skills/project-context/SKILL.md)) — follow that skill whenever you read, update, or audit the context.

**Settings** ([config.md](.agents/context/config.md)): update mode **automatic** · detail **standard** · scope **project + domains** · template **none (generic)**.

**Layout**

| File | Holds |
|---|---|
| [README.md](.agents/context/README.md) | Entry point; which files to read per task type |
| [config.md](.agents/context/config.md) | Maintenance settings |
| [structure.md](.agents/context/structure.md) | What each context file is for, what must not go in it, when to create new files |
| [architecture.md](.agents/context/architecture.md) | Services, data flow, write ownership, stack |
| [conventions.md](.agents/context/conventions.md) | Naming, repo layout, tooling, testing, git rules |
| [database.md](.agents/context/database.md) | Data categories, RLS and migration rules |
| [api.md](.agents/context/api.md) | API surfaces, auth per surface, contract rules |
| [workflows.md](.agents/context/workflows.md) | Event lifecycle, device login, recalculation, corrections, submissions |
| [domains/](.agents/context/domains/README.md) | `time-calculation`, `privacy`, `tenancy` |

**How to use it**

- **Read before substantial work:** `README.md → config.md → structure.md →` only the files relevant to your task `→` source code.
- **Source of truth:** actual code > context > blueprint/plans. If context disagrees with code, trust the code and fix the context — never change code to match stale context.
- **Update automatically** after any change that alters architecture, APIs, data model, auth, workflows, infrastructure, or domain rules: edit only the smallest set of affected files in the same task, then check for contradictions. Update `README.md` if navigation changes and `structure.md` if the organization changes.
- **Ignore** formatting, typos, renames, and internal refactors.
- **`Status: planned` markers:** the context currently describes the planned design. When you implement a part, verify the context against the code, correct it, and remove the marker.
- **Don't duplicate:** context links to the blueprint and plans; it never copies task lists, progress, or code inventories (files, functions, columns, endpoints).
- **User overrides:** "update the context" → update; "do not update context" → skip; "audit the context" → run the skill's audit procedure.

## Git

- Default branch `main`. Work on branches named `<service>/<TASK-ID>-short-name` (e.g., `engine/EN-7-overlap`).
- Commit messages: `<service>: <summary> (<TASK-ID>)`.
- Commit or push only when the user asks.
