# Progress — Shared (`shared/`)

> **Plan:** [SHARED.md](../plans/SHARED.md) · **Overall:** [GENERAL.progress.md](GENERAL.progress.md)
>
> Tick only when acceptance criteria are met. Format: `- [x] ID — task · YYYY-MM-DD · note`. In progress: `· 🚧`. Blocked: `· ⛔ reason`.

## Phase 1

### M0
- [ ] SH-1 — Create crate, wire into workspace, CI
  - Acceptance: builds in CI on 3 OSes

### M1
- [ ] SH-2 — IDs + UUIDv7
  - Acceptance: time-sortable IDs; serde round-trip
- [ ] SH-3 — Event envelope + Phase 1 event types (`presence.*`, `window.focus`, `editor.*`, `git.*`, `agent.*`)
  - Acceptance: serde round-trip tests for every type
- [ ] SH-4 — Validation
  - Acceptance: rejects missing fields, end < start, future > 5 min skew, metadata > 8 KB, unknown type
- [ ] SH-5 — Redaction engine + default rules
  - Acceptance: redaction corpus (≥ 100 secret samples) — zero survive
- [ ] SH-6 — URL stripping and remote URL normalization
  - Acceptance: table-driven tests incl. edge cases
- [ ] SH-7 — Device API types (auth config, register, batch, heartbeat, config)
  - Acceptance: used by agent and warehouse without duplication
- [ ] SH-8 — AgentConfig + Appendix A defaults
  - Acceptance: defaults match blueprint
- [ ] SH-9 — TS type generation (`ts-rs`) → `agent/vscode`, `overview/web`
  - Acceptance: CI fails if generated types are stale

## Phase 2
- [ ] SH-20 — Event types: `terminal.command`, `browser.tab_focus`, `fs.change`
- [ ] SH-21 — Terminal argument opt-in redaction path
- [ ] SH-22 — `schema_version` 2 helpers (accept N and N-1)

## Phase 3
- [ ] SH-30 — Team config types (shared project link hints, workspace policies)

## Notes
