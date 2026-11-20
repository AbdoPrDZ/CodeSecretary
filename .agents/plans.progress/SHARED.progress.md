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
- [x] SH-4 — Validation · 2026-11-20 · `validate.rs`: `parse` (untrusted JSON → precise reason) + `validate` (typed); reason codes = warehouse `rejected[].reason`; 13 tests cover missing fields, end < start, future > 5 min, metadata > 8 KB (UTF-8 bytes), unknown type, plus interval/point shape, repo-relative editor paths, commit hashes, backfill `estimated`; warehouse `ingest.py` mirrors the new rules (test added)
  - Acceptance: rejects missing fields, end < start, future > 5 min skew, metadata > 8 KB, unknown type
- [x] SH-5 — Redaction engine + default rules · 2026-11-20 · `redact/`: `redact(&mut Event)` per field (titles: URLs stripped, secrets + e-mails masked, ≤ 200 chars; remotes normalized; other text secret-masked; hashes/enums untouched); ~35 patterns (PEM keys, URL credentials, JWT, AWS, GitHub, GitLab, Slack, Discord, Stripe, Google, OpenAI/Anthropic, npm, PyPI, SendGrid, Twilio, DO, Shopify, HF, Azure, auth headers, `KEY=value`, high-entropy); corpus `shared/fixtures/redaction-corpus-v1.json`: 145 secrets, zero survive, 12 benign titles unchanged
  - Acceptance: redaction corpus (≥ 100 secret samples) — zero survive
- [x] SH-6 — URL stripping and remote URL normalization · 2026-11-20 · `redact/url.rs`: `strip_url` (no userinfo/query/fragment, token-like segments → `:id`), `normalize_remote` (scp, ssh, https, git, Azure `v3/` → `host/path`; local paths → none); 36 table-driven cases
  - Acceptance: table-driven tests incl. edge cases
- [ ] SH-7 — Device API types (auth config, register, batch, heartbeat, config) · 🚧 in progress · 2026-11-07
  - Acceptance: used by agent and warehouse without duplication
- [x] SH-8 — AgentConfig + Appendix A defaults · 2026-11-20 · `config.rs`: `defaults` (all Appendix A values, typed), `AgentConfig` (serde defaults on every field, so old and new agents interoperate), `Exclusions` (path globs / app names / domains incl. subdomains; case-insensitive, `\` = `/`), `PauseWindow` (`HH:MM`, wraps midnight, ISO weekdays); 7 tests incl. "defaults match Appendix A"
  - Acceptance: defaults match blueprint
- [ ] SH-9 — TS type generation (`ts-rs`) → `agent/vscode`, `overview/web`
  - Acceptance: CI fails if generated types are stale

## Phase 2
- [ ] SH-20 — Event types: `terminal.command`, `browser.tab_focus`, `fs.change`
- [ ] SH-21 — Terminal argument opt-in redaction path
- [ ] SH-22 — `schema_version` 2 helpers (accept N and N-1)
- [ ] SH-23 — Kotlin types + validation for Android from the JSON Schema (D16)

## Phase 3
- [ ] SH-30 — Team config types (shared project link hints, workspace policies)

## Notes
- 2026-11-07: Added `agent.buffer_overflow` (AGENT plan §3.1) to the Phase 1 catalog; it was missing from blueprint Appendix B.
- 2026-11-07: Until SH-9 generates a JSON Schema, the warehouse mirrors the envelope and API bodies by hand (`secretary_warehouse/ingest.py`, `api.py`). SH-7 stays open until that duplication is gone.
- 2026-11-20: SH-4 added rules beyond the acceptance:
  - Interval types need `timestamp_end`; point types must not span time.
  - Editor `file_path` must be repository-relative: no absolute, `~` or `..` paths.
  - Commit hashes are 7–64 hex characters, and `git.backfill_commit.estimated` must be true.
  - New reason codes: `missing_field`, `invalid_field`.
  - The warehouse now measures metadata size in UTF-8 bytes. Before, Python's ASCII escaping made non-ASCII metadata count up to 3× larger than in the agent.
- 2026-11-20 (SH-5): The corpus stores each fake secret split into parts, joined by the test, so the repo never holds a contiguous token (no secret-scanner or push-protection hits). Regenerate it with `shared/fixtures/gen_redaction_corpus.py`.
  - **Deviation:** `redaction_version` is not written into each event's metadata (plan §4.2). It is in `AgentConfig`, and adding a metadata field would change the contract for every type. Revisit with SH-22.
  - **Over-redaction is accepted:** long hex or base64 strings in titles (e.g. full Git hashes) are masked, and so is any value after a key ending in key/token/secret/password/auth.
