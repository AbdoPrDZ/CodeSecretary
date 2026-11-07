# Progress — Agent (`agent/`)

> **Plan:** [AGENT.md](../plans/AGENT.md) · **Overall:** [GENERAL.progress.md](GENERAL.progress.md)
>
> Tick only when acceptance criteria are met. Format: `- [x] ID — task · YYYY-MM-DD · note`. In progress: `· 🚧`. Blocked: `· ⛔ reason`.

## Phase 0 — Validation
- [ ] AG-0.1 — Presence + focus prototype (Windows, macOS, Linux X11; probe Wayland) · 🚧 in progress · 2026-11-06 · `secretary-agent probe` built; verified on Windows; macOS/Linux compile (clippy clean) but need a manual run on each OS
  - Acceptance: CLI prints idle state and focused app/title correctly on each OS
- [ ] AG-0.2 — Evaluate ActivityWatch watcher code for reuse (OD1)
  - Acceptance: decision recorded in GENERAL.md §10

## Phase 1

### M0
- [ ] AG-1 — Daemon skeleton: single instance, logging, config dir, graceful shutdown · 🚧 in progress · 2026-11-06 · implemented; 15 tests + fmt + clippy pass on Windows; macOS/Linux await first CI run
  - Acceptance: starts/stops cleanly on 3 OSes

### M1
- [ ] AG-2 — Keycloak device login, refresh token in OS keychain, token refresh, device registration
  - Acceptance: approve in browser; stays signed in across restarts; revocation → re-login prompt
- [ ] AG-3 — SQLite buffer
  - Acceptance: survives crash; bounded; FIFO; overflow tests
- [ ] AG-4 — Uploader (batch, gzip, retry, ack, clock offset)
  - Acceptance: offline 1 h → all events delivered exactly once on reconnect
- [ ] AG-5 — Heartbeat
  - Acceptance: every 60 s with version, queue depth, clock offset
- [ ] AG-6 — Pipeline: validate → exclusions → redact → buffer
  - Acceptance: redaction corpus passes on the agent path
- [ ] AG-7 — Config fetch + cache, apply exclusions and pause
  - Acceptance: dashboard change applies within 5 min

### M2
- [ ] AG-8 — Presence collector (Windows, macOS, Linux X11; Wayland best-effort)
  - Acceptance: idle intervals match manual test script ±10 s
- [ ] AG-9 — Focus collector
  - Acceptance: focus intervals correct; titles redacted
- [ ] AG-10 — Git tracker (discover, watch, remote normalize)
  - Acceptance: commits/checkouts appear within 10 s
- [ ] AG-11 — Git backfill
  - Acceptance: N weeks imported once; idempotent re-run
- [ ] AG-12 — Local API for extensions
  - Acceptance: rejects missing token and any `Origin` header
- [ ] AG-13 — VS Code extension v1
  - Acceptance: all Phase 1 editor events emitted; no file contents ever sent
- [ ] AG-14 — Tray: status, today total, pause 15 min / 1 h / resume, sign out
  - Acceptance: pause stops collection immediately and emits `agent.pause`

### M5
- [ ] AG-15 — Packaging: Windows MSI, macOS signed pkg, Linux deb + AppImage; start at login
  - Acceptance: clean install/uninstall on each OS
- [ ] AG-16 — Auto-update (signed manifest)
  - Acceptance: update applies without losing buffered events
- [ ] AG-17 — Performance check
  - Acceptance: < 1% CPU avg, < 80 MB RAM over a workday

### Security checklist
- [ ] Refresh token only in OS keychain; access token only in memory
- [ ] Local API bound to 127.0.0.1, token required, `Origin` rejected
- [ ] Local port/token file readable by current user only
- [ ] Redaction before buffer write
- [ ] No event metadata in logs
- [ ] Signed binaries and updates

## Phase 2
- [ ] AG-20 — Shell hooks: Bash, Zsh, Fish, PowerShell → local API (command name only)
- [ ] AG-21 — Installer step to add/remove shell hooks safely
- [ ] AG-22 — VS Code integrated terminal tagging + dedupe flag
- [ ] AG-23 — Browser extension (Chromium + Firefox)
- [ ] AG-24 — Native messaging host registration per OS/browser
- [ ] AG-25 — File system tracker with ignore rules
- [ ] AG-26 — Local "what was collected" viewer

## Phase 3
- [ ] AG-30 — Show team project links in tray
- [ ] AG-31 — Respect workspace-level policies pushed via config

## Notes
