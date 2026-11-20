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
- [ ] AG-1 — Daemon skeleton: single instance, logging, config dir, graceful shutdown · 🚧 in progress · 2026-11-06 · implemented; 16 unit + 4 lifecycle tests + fmt + clippy pass on Windows; macOS/Linux await first CI run (branch not pushed yet)
  - Acceptance: starts/stops cleanly on 3 OSes

### M1
- [x] AG-2 — Keycloak device login, refresh token in OS keychain, token refresh, device registration · 2026-11-14 · `login`/`logout`/`status`; device grant against Compose Keycloak, refresh token in Windows Credential Manager (`keyring`), access token in memory, refresh with rotation persisted, `POST /v1/devices`; E2E on Windows: signed in across 2 restarts, revoked device → agent signs out and `status` asks to log in, re-login → new device; macOS Keychain / Linux Secret Service compile-only so far
  - Acceptance: approve in browser; stays signed in across restarts; revocation → re-login prompt
- [x] AG-3 — SQLite buffer · 2026-11-14 · `buffer.rs` (rusqlite bundled, WAL): FIFO, dedupe on event_id, ack-delete, 30 days / 500k rows bound with an `agent.buffer_overflow` marker; tests: reopen without close (crash), overflow, age, corrupt row
  - Acceptance: survives crash; bounded; FIFO; overflow tests
- [ ] AG-4 — Uploader (batch, gzip, retry, ack, clock offset) · 🚧 in progress · 2026-11-14 · implemented: 60 s / 500-event trigger, gzip batches ≤ 500 (halved if > 1 MB), ack accepted + duplicates + rejected, 401 → refresh once → retry, 403 → re-register, backoff 1 s → 5 min with jitter; 8 mock-server tests; E2E: warehouse down 30 s → all events delivered once (8 rows, 8 distinct ids); left: the 1 h offline run from the acceptance
  - Acceptance: offline 1 h → all events delivered exactly once on reconnect
- [x] AG-5 — Heartbeat · 2026-11-14 · every 60 s (first after 1 s): version, queue depth, clock offset; offset = round-trip midpoint − `server_time`; E2E: devices.last_seen_at and queue_depth updated
  - Acceptance: every 60 s with version, queue depth, clock offset
- [ ] AG-6 — Pipeline: validate → exclusions → redact → buffer · 🚧 in progress · 2026-11-20 · `pipeline.rs`: validate (shared) → exclusions → pause windows → buffer; presence uses it; 8 tests; left: redact step + corpus test on the agent path (needs SH-5)
  - Acceptance: redaction corpus passes on the agent path
- [x] AG-7 — Config fetch + cache, apply exclusions and pause · 2026-11-20 · `remote_config.rs`: `GET /v1/agent/config` every 5 min with `If-None-Match`, cached in `agent_config.json` (used from startup, offline and signed out), `PauseWatch` emits `agent.pause`/`agent.resume` (`exclusion_window`); pipeline applies path/app exclusions and cuts intervals at pause edges; tests: 3 cache + 8 pipeline + 2 mock-server; E2E on Windows + Compose: pause 14:33–14:35 local → pause/resume stored, presence interval cut into 13:32:45–13:33:00 and 13:35:00–13:37:45 UTC; exclusion added while running → applied after 4 min 44 s
  - Acceptance: dashboard change applies within 5 min

### M2
- [ ] AG-8 — Presence collector (Windows, macOS, Linux X11; Wayland best-effort) · 🚧 in progress · 2026-11-14 · `presence.rs`: 5 s poll, idle after 60 s back-dated to last input (never before collector start), 5 min checkpoints, sleep-gap handling, flush on shutdown; 6 state-machine tests; E2E on Windows: intervals stored in Postgres; left: manual ±10 s script on each OS
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
- [x] Refresh token only in OS keychain; access token only in memory · 2026-11-14 · state.json, logs and stdout checked: no tokens; keychain entry removed on logout and sign-out
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
- [ ] AG-27 — Android: skeleton, sign-in (AppAuth + PKCE), device registration
- [ ] AG-28 — Android: usage-events collector, Room buffer, uploader, heartbeat
- [ ] AG-29 — Android: config fetch, companion screens (today, review, pause, collected)

## Phase 3
- [ ] AG-30 — Show team project links in tray
- [ ] AG-31 — Respect workspace-level policies pushed via config

## Notes
- 2026-11-14: Daemon layout is flatter than plan §3 for now: `auth/` (oidc, store, `Link`), `api.rs`, `buffer.rs`, `uploader.rs`, `heartbeat.rs`, `presence.rs`, `state.rs`, `login.rs`. Split into `collectors/` when the focus and git collectors land.
- 2026-11-14: Presence events go straight to the buffer: they carry no metadata, so the AG-6 pipeline (validate → exclusions → redact) is not needed for them yet. Focus (AG-9) must wait for SH-5 redaction. (Superseded 2026-11-20: presence now goes through the pipeline.)
- 2026-11-14: Collector idle threshold is 60 s; the engine applies the user's idle threshold (Appendix A, 5 min) on top.
- 2026-11-14: The keychain entry is per installation (`CodeSecretary` / `refresh-token/<installation_id>` from state.json), so separate agent homes never share a token. The daemon never touches the keychain until `login` has written state.json.
- 2026-11-14: When the warehouse reports a revoked device, the agent deletes its keychain entry but does not end the Keycloak session; overview's revoke (OV-4) ends it. Events buffered while signed out are uploaded under the next device registration.
- 2026-11-14: Linux builds need `libdbus-1-dev` (keyring's Secret Service backend); added to CI.
- 2026-11-20 (AG-6/AG-7): The pipeline runs validate → exclusions → **pause** → (redact) → buffer.
  - **Pause windows:** applied in device local time. Point events inside a window are dropped. Intervals are cut at the window edges: the first piece keeps the event id, later pieces get new ones. `agent.*` events are exempt, so the `agent.pause` / `agent.resume` markers themselves are kept.
  - **Exclusions:** an event is dropped whole if its repository root, working directory, workspace path, root + file path, or focused app matches. A leading `~` in path patterns is expanded to the home directory.
  - **Config cache:** `agent_config.json` survives `logout`, because events buffered while signed out are uploaded later and must still be filtered.
  - **Not yet driven by config:** upload, heartbeat and edit-aggregation timings are still the built-in constants (equal to the defaults), and `project_roots` is unused until AG-10.
