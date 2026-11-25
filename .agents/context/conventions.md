# Conventions

Status: partially implemented. The root Cargo workspace (edition 2024, shared `[workspace.dependencies]`), `agent/daemon` and CI (`.github/workflows/ci.yml`: fmt + clippy `-D warnings` + tests on Linux/macOS/Windows) exist. Everything else is planned. Source: [GENERAL plan](../plans/GENERAL.md) §3, §7, §9.

## Naming

- Product: **CodeSecretary**. Short technical name: `secretary` (binaries, crates, packages, config dirs). Avoid `codesec` (reads as "code security").
- Crate names: `secretary-shared`, `secretary-agent`, `secretary-warehouse`, `secretary-engine`, `secretary-overview-api`.

## Repository Layout

```
Cargo.toml (workspace) · docker-compose.yml · .github/workflows/
shared/ · agent/{daemon,vscode,browser,shell}/ · warehouse/{src/secretary_warehouse/schema,tests}/ · engine/ · overview/{api,web}/ · orm/ (Python package `secretary-orm`)
.agents/{blueprints,plans,plans.progress,context,skills}/
```

## Tooling

- **Rust** (agent, engine, shared): one Cargo workspace; tokio, sqlx (engine), serde, tracing, thiserror/anyhow. `cargo fmt` + `cargo clippy -D warnings` must pass.
- **Python** (warehouse, overview API, orm): 3.13, FastAPI + uvicorn, `secretary-orm`, exact pins in `pyproject.toml`, 2-space indent (J-ERP style), pytest against real Postgres.
- **TypeScript:** pnpm workspaces; strict TS, ESLint, Prettier. Types shared with Rust are **generated** from `shared/` (`ts-rs`), never hand-copied.

## Rules Agents Must Follow

- Contracts between services change only through `shared/` + the warehouse schema.
- Every new tenant table gets an RLS policy in `post.sql` **in the same change as its model**, and an isolation test.
- Any change to engine output requires updated golden fixtures and, if intentional, an `ALGORITHM_VERSION` bump.
- Any event schema change bumps `schema_version`; warehouse accepts N and N-1.
- **Never log event metadata** (titles, URLs, paths, commands) — IDs and counts only.
- Never send file contents, keystrokes, or clipboard anywhere.

## Testing

Unit · integration (testcontainers Postgres + MinIO) · engine golden tests (`engine/fixtures/<case>/{context,expected}.json`; inputs from `gen_fixtures.py`, outputs blessed with `SECRETARY_BLESS=1` and reviewed by hand) and engine DB tests (`ENGINE_TEST_DATABASE_URL` + `ENGINE_TEST_APP_URL`) · tenant isolation tests · redaction corpus · E2E on Docker Compose. Details: GENERAL plan §7.4.

## Definition of Done

Tests + CI green · contracts/schema updated · no metadata in logs · RLS + isolation test for new tables · progress file updated (see `AGENTS.md`).

## Git

- Default branch `main`; work on feature branches `<service>/<task-id>-short-name` (e.g., `agent/AG-8-presence`).
- Commit only when the user asks.
- Commit messages: `<service>: <summary> (<TASK-ID>)`, e.g. `engine: overlap resolution (EN-7)`.
- Line endings normalized to LF via `.gitattributes` (PowerShell/batch scripts keep CRLF).
