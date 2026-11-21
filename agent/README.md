# CodeSecretary Agent

The local agent. It collects activity evidence, redacts it, buffers it, and sends it to the warehouse. It never calculates time.

| Folder | What | Status |
|---|---|---|
| `daemon/` | Rust daemon `secretary-agent` | Keycloak sign-in, presence collector, pipeline with exclusions and pause windows, SQLite buffer, uploader, heartbeat, remote config (AG-1–8) |
| `panel/` | `codesecretary`: desktop widget + tray icon; starts the daemon | Status, sign-in, start/stop (AG-14, part) |
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
5. Every event passes the pipeline before it is buffered: validation, then the user's **exclusions** (paths, apps) and **pause windows** (local time; intervals are cut at the edges, with `agent.pause` / `agent.resume` markers). The rules come from `GET /v1/agent/config`, refetched every 5 min and cached in `<data>/agent_config.json` so they also apply offline.

`<data>/state.json` holds the sign-in settings and the device id, but no tokens. Linux builds need `libdbus-1-dev` and `pkg-config`.

### CodeSecretary widget (AG-14)

`codesecretary` (`agent/panel`) is the program to start. It:

1. **Starts the daemon** (`secretary-agent run`, which must sit next to it) as a separate background process with no console window, unless one is already running. Closing the widget leaves the daemon running.
2. **Shows a floating desktop widget.** It's a small rounded, semi-transparent card you can drag anywhere. It remembers its position and can stay on top. It shows:
   - the status: connected, signed out, server unreachable, agent stopped
   - the account, events waiting, the last upload, active/idle, and the rules in use
   - a **Details** section with the server, heartbeat, clock offset and agent version
3. **Adds a tray icon** whose dot is green, amber, red or grey with the status. Left-click shows or hides the widget. The menu has:
   - Sign in / Sign out
   - Keep on top
   - Open data folder
   - Start / Stop agent
   - Close widget
4. **Signs in from the widget.** **Sign in…** asks for the server URL, then runs `secretary-agent login --json`: the browser opens, and the widget shows the device code until it's approved.

How they talk: the daemon rewrites `<data>/status.json` every 5 s and the widget reads it every 2 s. The file holds states, counts and times only, never event contents. The daemon runs no server for it. `secretary-agent stop` (also in the menu) asks a running daemon to exit by dropping `<data>/stop.request`, which works the same on every OS.

```sh
cargo build --release -p secretary-agent -p secretary-panel
target/release/codesecretary          # .exe on Windows; starts the daemon too
```

Building on Linux needs `libgtk-3-dev`, `libayatana-appindicator3-dev`, `libxkbcommon-dev` and `libgl1-mesa-dev` for the window and the tray icon.

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
