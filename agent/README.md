# CodeSecretary Agent

The local agent. It collects activity evidence, redacts it, buffers it, and sends it to the warehouse. It never calculates time.

| Folder | What | Status |
|---|---|---|
| `daemon/` | Rust daemon `secretary-agent` | Keycloak sign-in, presence collector, SQLite buffer, uploader, heartbeat (AG-1–5, AG-8) |
| `vscode/` | VS Code extension | Planned (AG-13) |
| `browser/` | Browser extension | Phase 2 |
| `shell/` | Shell hooks | Phase 2 |

Plan: [.agents/plans/AGENT.md](../.agents/plans/AGENT.md) · Progress: [.agents/plans.progress/AGENT.progress.md](../.agents/plans.progress/AGENT.progress.md)

## Daemon

```sh
cargo run -p secretary-agent -- login    # sign this device in (browser + code, Keycloak device flow)
cargo run -p secretary-agent             # run in the foreground (Ctrl+C to stop)
cargo run -p secretary-agent -- status   # signed in? device id, events waiting for upload
cargo run -p secretary-agent -- logout   # end the session, remove the token from the keychain
cargo run -p secretary-agent -- paths    # show config/data/log directories
cargo run -p secretary-agent -- probe    # live idle/active + focused app/title (Phase 0 prototype)
cargo test -p secretary-agent
```

| Setting | Where |
|---|---|
| Bootstrap config | `agent.toml` in the config dir: `server_url` (default `http://localhost:8080`), `log_level` (default `info`) |
| Log level override | `SECRETARY_LOG=debug` |
| Use a different root for all dirs (dev/tests) | `SECRETARY_HOME=<dir>` → `<dir>/{config,data,logs}` |

### Sign-in and upload

Start the local stack first (`docker compose up -d --build --wait`; dev user `dev@example.test` / `dev`).

1. `login` asks the warehouse where to sign in (`GET /v1/auth/config`), starts Keycloak's device authorization grant, prints a link and a code, and opens the browser (`--no-browser` to skip). After you approve, it stores the **offline refresh token in the OS keychain** (Windows Credential Manager, macOS Keychain, Linux Secret Service) and registers the device (`POST /v1/devices`).
2. `run` collects `presence.active` / `presence.idle` intervals into the local buffer (`<data>/buffer.sqlite3`). The uploader sends them every 60 s (or at 500 events) as gzip batches to `POST /v1/events:batch` and deletes them only once acknowledged. A heartbeat every 60 s reports version, queue depth and clock offset.
3. Access tokens live only in memory and are refreshed before they expire; Keycloak rotates the refresh token each time.
4. If the device is revoked or the session ends, the agent signs itself out and logs `run secretary-agent login`. Collection continues into the buffer, and a new `login` resumes uploading without a restart.

`<data>/state.json` holds the sign-in settings and the device id, but no tokens. Linux builds need `libdbus-1-dev` and `pkg-config`.

### Probe (AG-0.1)

`probe` prints presence and focus changes live, to the terminal only. Nothing is logged, stored, or sent.

```sh
cargo run -p secretary-agent -- probe --idle-after 10   # count as idle after 10 s (default 300)
cargo run -p secretary-agent -- probe --no-titles       # app names only
```

| OS | Idle | Focused app | Window title |
|---|---|---|---|
| Windows | yes | yes | yes |
| macOS | yes | yes | only with Accessibility permission for the terminal |
| Linux X11 | yes | yes | yes |
| Linux Wayland | through XWayland only (X11 apps) | same | same |

Exit codes: `0` ok · `1` failure · `2` invalid config · `3` another instance is already running.
