# Progress — Shared (`shared/`)

> **Plan:** [SHARED.md](../plans/SHARED.md) · **Overall:** [GENERAL.progress.md](GENERAL.progress.md)
>
> Tick only when acceptance criteria are met. Format: `- [x] ID — task · YYYY-MM-DD · note`. In progress: `· 🚧`. Blocked: `· ⛔ reason`.

## Phase 1

### M0
- [ ] SH-1 — Create crate, wire into workspace, CI · 🚧 in progress · 2026-11-07 · `secretary-shared` in the workspace, builds + tests on Windows; needs the CI run on 3 OSes
  - Acceptance: builds in CI on 3 OSes

### M1
- [x] SH-2 — IDs + UUIDv7 · 2026-11-07 · `EventId` (UUIDv7, transparent serde); test: 1000 ids sort in generation order
  - Acceptance: time-sortable IDs; serde round-trip
- [x] SH-3 — Event envelope + Phase 1 event types (`presence.*`, `window.focus`, `editor.*`, `git.*`, `agent.*`) · 2026-11-07 · flat envelope, `Payload` adjacently tagged (`event_type` + `metadata`), `source` as `name@version`; round-trip test for all 16 types
  - Acceptance: serde round-trip tests for every type
- [ ] SH-4 — Validation
  - Acceptance: rejects missing fields, end < start, future > 5 min skew, metadata > 8 KB, unknown type
- [ ] SH-5 — Redaction engine + default rules
  - Acceptance: redaction corpus (≥ 100 secret samples) — zero survive
- [ ] SH-6 — URL stripping and remote URL normalization
  - Acceptance: table-driven tests incl. edge cases
- [ ] SH-7 — Device API types (auth config, register, batch, heartbeat, config) · 🚧 in progress · 2026-11-07
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
- 2026-11-07: Added `agent.buffer_overflow` (AGENT plan §3.1) to the Phase 1 catalog; it was missing from blueprint Appendix B.
- 2026-11-07: Until SH-9 generates a JSON Schema, the warehouse mirrors the envelope and API bodies by hand (`secretary_warehouse/ingest.py`, `api.py`). SH-7 stays open until that duplication is gone.
