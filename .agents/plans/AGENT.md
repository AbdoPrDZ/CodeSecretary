# Service Plan — Agent (`agent/`)

> **Blueprint:** [IDEA-V4.md](../blueprints/IDEA-V4.md) §5.1, §5.3, §8, §15, Appendix A–C · **General plan:** [GENERAL.md](GENERAL.md)

## 1. Purpose

The only component that runs on the developer's machine. It **collects, redacts, buffers, and uploads** activity evidence. It never calculates time.

## 2. Components

| Component | Folder | Language | Phase |
|---|---|---|---|
| Daemon + tray | `agent/daemon/` | Rust | 1 |
| VS Code extension | `agent/vscode/` | TypeScript | 1 |
| Browser extension | `agent/browser/` | TypeScript (WebExtensions) | 2 |
| Shell hooks | `agent/shell/` | Bash, Zsh, Fish, PowerShell | 2 |
| Android app (collector + companion, D16) | `agent/android/` | Kotlin, Jetpack | 2 |

## 3. Daemon Architecture

```
agent/daemon/src/
├─ main.rs              # startup, single-instance lock, signal handling
├─ auth/                # Keycloak device authorization grant, token refresh, refresh token in OS keychain
├─ config/              # fetch + cache AgentConfig; pause state; exclusions
├─ collectors/
│  ├─ presence/         # idle detection — windows.rs, macos.rs, linux_x11.rs, linux_wayland.rs
│  ├─ focus/            # focused app + title — per-OS modules
│  ├─ git/              # repo discovery, watcher, backfill
│  └─ fs/               # Phase 2
├─ local_api/           # 127.0.0.1 server for VS Code ext + shell hooks; per-collector tokens
├─ native_messaging/    # Phase 2: browser host
├─ pipeline/            # validate → exclusions → redact → enqueue
├─ buffer/              # SQLite queue (rusqlite), bounded 30 days
├─ uploader/            # batching, gzip, retry/backoff, clock offset
├─ heartbeat/
├─ tray/                # tray icon: current project, today total, pause, sync status
└─ update/              # signed auto-update
```

### 3.1 Event Pipeline

```
collector ─► pipeline: validate (shared) ─► exclusions ─► redact (shared) ─► buffer (SQLite)
                                                                               │
                                     uploader ◄── every 60 s or 500 events ────┘
                                        │ gzip batch, access token
                                        ▼
                                  warehouse /v1/events:batch
                                        │ ack event_ids
                                        ▼
                                  delete acked from buffer
```

- Events are written to the buffer **after** redaction — raw secrets never touch disk.
- On 401: refresh the access token once and retry; if the refresh fails (`invalid_grant`: revoked or expired session), stop uploading and show "Sign in again" in tray.
- On 5xx / network error: exponential backoff (1 s → 5 min), jitter.
- Buffer full (30 days): drop oldest, emit `agent.buffer_overflow` event.

### 3.2 Presence and Focus

| OS | Idle | Focus / title |
|---|---|---|
| Windows | `GetLastInputInfo` | `GetForegroundWindow` + `GetWindowTextW` + process name |
| macOS | `CGEventSourceSecondsSinceLastEventType` | `NSWorkspace.frontmostApplication`; title via Accessibility API (permission) |
| Linux X11 | XScreenSaver extension | `_NET_ACTIVE_WINDOW`, `_NET_WM_NAME` |
| Linux Wayland | `ext-idle-notify-v1` where available; else D-Bus `org.gnome.Mutter.IdleMonitor` | Best-effort; app only, no title by default |

- Poll every 5 s; emit `presence.active` / `presence.idle` **intervals** (state changes), not polls.
- Emit `window.focus` intervals on focus change (debounced 2 s).
- Lock screen / sleep → force idle.

### 3.3 Git Tracker

- Discover repos under configured project roots (depth-limited, skip ignored dirs).
- Watch `.git/HEAD`, `.git/refs/`, `.git/logs/HEAD` for changes → `git.commit`, `git.checkout`, `git.merge`.
- Read remote URL; normalize via `shared`.
- **Backfill:** on request, read commit history (last N weeks, author = configured emails) → `git.backfill_commit` with `estimated: true`.
- Use `gix` (gitoxide) to avoid shelling out.

### 3.4 Local API (for VS Code extension and shell hooks)

- Bind `127.0.0.1` on a random free port; write port + per-collector token to a user-only file (`local.json` in the agent config dir: Linux `~/.config/secretary/`, macOS `~/Library/Application Support/secretary/`, Windows `%APPDATA%\secretary\config\`).
- `POST /local/v1/events` — same envelope as shared; agent assigns nothing but validates and pipelines.
- Reject requests with an `Origin` header (blocks websites); require bearer token.

### 3.5 Android App (`agent/android/`, Phase 2, D16)

One app that is both a collector and a companion, with the same contract as the daemon. iOS is out of scope.

- **Sign-in:** Keycloak authorization code + PKCE in a Custom Tab (AppAuth), client `secretary-android`, scope `openid offline_access`. The refresh token is encrypted with an Android Keystore key, and the access token stays in memory. Then `POST /v1/devices` with `os = "android"`.
- **Collection:** batch, not polling. A `WorkManager` job (about every 15 min, and when the app opens) reads `UsageStatsManager.queryEvents` since its last checkpoint.
  - Screen interactive / keyguard events → `presence.active` / `presence.idle` intervals.
  - App resumed / paused → `window.focus` intervals with `app` = package name and `title` = none.
  - Source `android@<version>`.
- **Permission:** "Usage access" (`PACKAGE_USAGE_STATS`) is granted by the user in system settings. Without it the app collects nothing and says so.
- **Pipeline:** the same steps as the daemon, in Kotlin: validate → exclusions (apps = package names) → pause windows → buffer. Validation follows the SH-9 JSON Schema. There is no redaction step, because no free text is collected.
- **Buffer and upload:** a Room (SQLite) FIFO with the daemon's rules: bounded, acknowledge before delete, the same gzip batches, heartbeat and clock offset.
- **Companion:**
  - today's total and sessions to review, from the overview API
  - pause / resume on this phone, emitting `agent.pause` / `agent.resume`
  - a "what was collected" view
- **Never:** accessibility services, notification listeners, screen capture, location, contacts, or any content.
- **Distribution:** a signed APK first (GitHub releases, F-Droid). Play Store review needs a justification for `PACKAGE_USAGE_STATS`; that is a later decision.

## 4. VS Code Extension (`agent/vscode/`)

- Activates on startup; reads local agent port/token file.
- Emits:
  - `editor.workspace_open` (workspace folders, remote URL if git)
  - `editor.file_focus` intervals (active editor + window focus state)
  - `editor.edit_burst` aggregated per 30 s per file (counts only, no content)
  - `editor.save`
  - `editor.debug_session` intervals
  - Integrated-terminal commands tagged `from_vscode: true` (Phase 2, via shell integration API)
- Paths sent **repo-relative** plus repository root.
- If agent unreachable: small in-memory buffer (5 min), then drop with a status-bar warning.
- Status bar item: current project + "Tracking paused" state.

## 5. Tasks

### Phase 0 — Validation

| ID | Task | Acceptance |
|---|---|---|
| AG-0.1 | Presence + focus prototype on Windows, macOS, Linux X11 (and probe Wayland) | CLI prints idle state and focused app/title correctly on each OS |
| AG-0.2 | Evaluate ActivityWatch watcher code for reuse (OD1) | Decision recorded in GENERAL.md §10 |

### Phase 1

| ID | Task | Milestone | Acceptance |
|---|---|---|---|
| AG-1 | Daemon skeleton: single instance, logging, config dir, graceful shutdown | M0 | Starts/stops cleanly on 3 OSes |
| AG-2 | Keycloak device login (OAuth 2.0 device grant, `offline_access`), refresh token in OS keychain (`keyring` crate), access-token refresh, device registration | M1 | User approves in browser; stays signed in across restarts; revocation → re-login prompt |
| AG-3 | SQLite buffer | M1 | Survives crash; bounded; FIFO; tests for overflow |
| AG-4 | Uploader (batch, gzip, retry, ack, clock offset) | M1 | Offline for 1 h → all events delivered once on reconnect |
| AG-5 | Heartbeat | M1 | Sent every 60 s with version, queue depth, clock offset |
| AG-6 | Pipeline: validate → exclusions → redact → buffer | M1 | Redaction corpus test passes on the agent path |
| AG-7 | Config fetch + cache (`GET /v1/agent/config`), apply exclusions and pause | M1 | Config change in dashboard applies within 5 min |
| AG-8 | Presence collector (Windows, macOS, Linux X11; Wayland best-effort) | M2 | Idle intervals match manual test script ±10 s |
| AG-9 | Focus collector | M2 | Focus intervals correct; titles redacted |
| AG-10 | Git tracker (discover, watch, remote normalize) | M2 | Commits/checkouts appear within 10 s |
| AG-11 | Git backfill | M2 | N weeks imported once; idempotent re-run |
| AG-12 | Local API for extensions | M2 | Rejects missing token and any `Origin` header |
| AG-13 | VS Code extension v1 | M2 | All Phase 1 editor events emitted; no file contents ever sent |
| AG-14 | Tray: status, today total (from overview API), pause 15 min / 1 h / resume, sign out | M2 | Pause stops collection immediately and emits `agent.pause` |
| AG-15 | Packaging: Windows installer (MSI), macOS signed `.pkg`/app, Linux `.deb` + AppImage; start at login | M5 | Clean install/uninstall on each OS |
| AG-16 | Auto-update (signed manifest) | M5 | Update applies without losing buffered events |
| AG-17 | Performance check | M5 | < 1% CPU avg, < 80 MB RAM over a workday |

### Phase 2

| ID | Task |
|---|---|
| AG-20 | Shell hooks: Bash (`PROMPT_COMMAND` + `DEBUG` trap), Zsh (`preexec`/`precmd`), Fish events, PowerShell (`prompt` / PSReadLine) → local API; command name only |
| AG-21 | Installer step that adds/removes shell hooks safely (idempotent, backed up rc files) |
| AG-22 | VS Code integrated terminal tagging + dedupe flag |
| AG-23 | Browser extension (Chromium + Firefox): focused tab intervals; no private windows; URL stripped at source |
| AG-24 | Native messaging host registration per OS/browser |
| AG-25 | File system tracker for project roots (`notify` crate) with ignore rules |
| AG-26 | Local "what was collected" viewer (tray → open last 100 events, redacted) |
| AG-27 | Android: project skeleton, sign-in (AppAuth + PKCE, Keystore-encrypted refresh token), device registration |
| AG-28 | Android: usage-events collector (presence + foreground app) with checkpointing, Room buffer, uploader, heartbeat |
| AG-29 | Android: config fetch (exclusions, pause windows), companion screens (today total, review, pause, collected events) |

### Phase 3

| ID | Task |
|---|---|
| AG-30 | Show team project links in tray (current project may be shared) |
| AG-31 | Respect workspace-level policies pushed via config (e.g., collectors disabled by team policy) |

## 6. Testing

- Unit: pipeline, buffer, uploader (mock server), remote normalization usage.
- OS integration: presence/focus test scripts per OS (manual checklist + automated where possible in CI runners: `windows-latest`, `macos-latest`, `ubuntu-latest`).
- Redaction: shared corpus run through the full agent pipeline.
- Chaos: kill daemon mid-upload; disconnect network; corrupt buffer file → recovery.
- VS Code extension: `@vscode/test-electron` tests.

## 7. Security Checklist

- [ ] Refresh token only in OS keychain; access token only in memory.
- [ ] Local API bound to 127.0.0.1, token required, `Origin` rejected.
- [ ] Local port/token file readable by current user only.
- [ ] Redaction before buffer write.
- [ ] No event metadata in logs.
- [ ] Signed binaries and updates.

## 8. Risks

| Risk | Mitigation |
|---|---|
| macOS Accessibility permission denied | Degrade: app name only; tray shows "limited tracking" |
| Wayland gaps | Document; app-level only; encourage VS Code ext |
| Antivirus flags (input idle APIs, local server) | Code signing; avoid low-level hooks — only idle-time APIs |
| Shell hook breaks user's shell | Hooks are no-op on error; uninstall restores backups |
| Android OEMs kill background work; usage events kept only ~days | Catch up when the app opens; checkpoint every run; show "last synced" |
| Phone time inflates project time | Engine credits phone focus only when mapped to a project (EN-28) |
