# Domain — Privacy & Security

Status: planned. Source: blueprint §3.2, §14; [SHARED plan](../../plans/SHARED.md) §4.2.

## Purpose

CodeSecretary handles sensitive activity data and may be used inside companies. Trust is the product's precondition: it must never become a surveillance tool or leak secrets.

## Never Collect

File contents · keystroke content · clipboard · screenshots/screen recording · private/incognito browser windows · productivity scores of people.

## Redaction (in the agent, before buffering)

Status: implemented (SH-5, SH-6; levels D18). Source: `shared/src/redact/`; corpus `shared/fixtures/redaction-corpus-v1.json`. The terminal rule is Phase 2 (SH-21).

- **Level (D18)** comes from the agent config (`redaction`):
  - `secrets` (default): credentials only. Keys, tokens, JWTs, passwords, private keys and URL user:password are masked; paths, titles, URLs and e-mail addresses are sent as collected.
  - `none`: nothing masked.
  - `full`: every rule below.
- **Remotes** are normalized at every level: they are the project-matching key, and normalizing drops embedded credentials.
- The rules below are the `full` level.
- Implemented once in `shared/` and used by the agent (and validated server-side).
- Terminal: command **name** only by default (e.g., `git push`, `npm run`); arguments are opt-in per project and still secret-masked.
- URLs: drop query, fragment, userinfo; token-like path segments → `:id`.
- Window titles: mask emails and secret patterns; truncate.
- Secret patterns (API keys, tokens, JWTs, `Bearer …`, `*_KEY=…`, credentialed connection strings, private key headers) are masked everywhere.
- Paths are sent repo-relative plus repository root. Validation (`shared/`, mirrored in the warehouse) rejects absolute, `~` and `..` editor paths.
- **Exclusions and pause windows** are applied in the agent before buffering, so excluded activity never reaches disk. Matching is case-insensitive and generous: over-matching only hides data. Status: implemented (AG-7).
- Redaction rules are versioned (`REDACTION_VERSION`); every change is validated by the redaction corpus test (zero survivors), in `shared/` and again through the agent pipeline.
- Git remotes are normalized to `host/owner/repo`, which also drops credentials.
- Over-masking is accepted (e.g. long hex strings in titles); under-masking is a bug.
- The corpus holds fake secrets split into parts, so the repository never contains a contiguous token.

## Visibility

Raw events, evidence, titles, URLs, commands, personal projects, and unsubmitted time are visible **only to their owner** — not teammates, not admins. Teams see submitted totals only. Enforced in the database (RLS), not just the UI. Details: [tenancy.md](tenancy.md).

## Security Rules

- Agent local API: `127.0.0.1`, per-collector token, reject any `Origin` header.
- Agent refresh token in the OS keychain, access token in memory only; revocation is immediate (warehouse checks the device) and ends the Keycloak session. Keycloak holds identity data only (email, name), never activity.
- TLS everywhere; encryption at rest; application-level encryption of sensitive metadata fields (Phase 2).
- **Never log event metadata.**
- AI is off by default; when on, only minimal redacted fields are sent and every request is logged for the user.
- Users can export and delete all their data.
