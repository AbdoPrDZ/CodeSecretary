# CodeSecretary Agent

The local agent. It collects activity evidence, redacts it, buffers it, and sends it to the warehouse. It never calculates time.

| Folder | What | Status |
|---|---|---|
| `daemon/` | Rust daemon `secretary-agent` | Skeleton (AG-1) |
| `vscode/` | VS Code extension | Planned (AG-13) |
| `browser/` | Browser extension | Phase 2 |
| `shell/` | Shell hooks | Phase 2 |

Plan: [.agents/plans/AGENT.md](../.agents/plans/AGENT.md) · Progress: [.agents/plans.progress/AGENT.progress.md](../.agents/plans.progress/AGENT.progress.md)

## Daemon

```sh
cargo run -p secretary-agent             # run in the foreground (Ctrl+C to stop)
cargo run -p secretary-agent -- paths    # show config/data/log directories
cargo run -p secretary-agent -- probe    # live idle/active + focused app/title (Phase 0 prototype)
cargo test -p secretary-agent
```

| Setting | Where |
|---|---|
| Bootstrap config | `agent.toml` in the config dir: `server_url` (default `http://localhost:8080`), `log_level` (default `info`) |
| Log level override | `SECRETARY_LOG=debug` |
| Use a different root for all dirs (dev/tests) | `SECRETARY_HOME=<dir>` → `<dir>/{config,data,logs}` |

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
