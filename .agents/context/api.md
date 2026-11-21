# APIs

Status: device API implemented (auth config, devices, batch ingest, heartbeat, agent config); user API partly implemented (sign-in, devices, activity, exclusions, projects and rules); other surfaces planned. Source: `warehouse/src/secretary_warehouse/{http,auth,ingest,agent_config}.py`, `overview/api/src/secretary_overview/`, `shared/src/{api,config,validate}.rs`; blueprint Appendix C; [WAREHOUSE plan](../plans/WAREHOUSE.md) §4; [AGENT plan](../plans/AGENT.md) §3.4; [OVERVIEW plan](../plans/OVERVIEW.md) §3.

## Surfaces

| Surface | Host | Callers | Auth |
|---|---|---|---|
| **Local agent API** (`/local/v1/*`) | agent, `127.0.0.1` random port | VS Code ext, shell hooks | Per-collector bearer token from a user-only file; requests with an `Origin` header rejected |
| **Native Messaging** (Phase 2) | agent | Browser extension | Browser-managed host registration |
| **Identity** (OIDC endpoints) | Keycloak, realm `secretary` | agent, overview | Agent: device authorization grant (`secretary-agent`, public client, offline token). Dashboard: auth code + PKCE |
| **Device API** (`/v1/auth/config`, `/v1/devices`, `/v1/events:batch`, `/v1/agent/*`) | warehouse | agent only | Keycloak access token (`aud` `secretary-warehouse`, `azp` `secretary-agent`); its `sid` must map to a registered, unrevoked device |
| **User API** | overview/api | overview/web | Keycloak OIDC sign-in → HTTP-only secure cookie session + CSRF |

## Device API Behaviour

Status: implemented (WH-4, WH-5, WH-8, WH-9). Source: `warehouse/src/secretary_warehouse/`.

- **Token checks:** RS256 signature against the realm JWKS (cached, refetched on an unknown `kid`), `iss` = configured issuer, `aud` ∋ `secretary-warehouse`, `azp` = `secretary-agent`, `typ` = `Bearer`, `exp`/`nbf`/`iat` with 30 s leeway, and `sub`, `sid`, `email` present.
- **Identity:** `sub` → user via `app_resolve_identity()` (first sight provisions the user + personal workspace). `sid` → device. Lookups are cached ≤ 60 s, so a revocation takes effect within a minute.
- **Errors** are always `{"error": code}`:
  - 401 `invalid_token` or `device_revoked`, with `WWW-Authenticate: Bearer`. The agent refreshes once, then signs out.
  - 403 `device_not_registered`. The agent registers, then retries.
  - 400 `invalid_body`. FastAPI's 422 is replaced because it echoes the input back.
  - 413 `batch_too_large`.
  - 503 `auth_unavailable`, when the JWKS cannot be fetched.
- **Batch limits:** ≤ 1 MB as sent (gzip or plain JSON), ≤ 16 MB inflated, ≤ 1,000 events. An invalid event is rejected on its own, with a reason code; the rest of the batch is stored.
- **Timestamps and clock offset:** `server_adjusted_*` = device time − `clock_offset_ms`. The offset is device − server clock; the agent measures it from the heartbeat's `server_time`.
- **Validation reasons:** `secretary_shared::validate` (SH-4) defines the rules and the reason codes reported in `rejected[].reason`. Interval types need an end, point types must not span time, and editor paths must be repository-relative.
- **Agent config:** `GET /v1/agent/config` returns `secretary_shared::config::AgentConfig`, built from the user's `exclusions` rows under RLS. The `ETag` is a hash of the body; `If-None-Match` gets 304. Unknown fields are ignored and missing ones take defaults on both sides, so agent and warehouse versions can differ.
- **Generated contract (SH-9):** `shared/src/codegen.rs` generates the files in `warehouse/src/secretary_warehouse/contract/`. The warehouse reads them, and a CI test fails when they are stale.
  - `*.schema.json`: the envelope, every device API body, and `AgentConfig`.
  - `constants.json`: event types, limits, reason/error codes, and defaults.
  - TypeScript types: `shared/bindings/ts/contract.ts`.
  - Regenerate after any contract change: `SECRETARY_BLESS=1 cargo test -p secretary-shared --features codegen generated`.
  - **Still by hand:** the semantic rules of `validate.rs` (timestamps, sizes, paths, hashes) are re-implemented in `ingest.py`. Change both together.

## User API Behaviour

Status: implemented for the first slice (OV-1–5, OV-12 data view). Source: `overview/api/src/secretary_overview/`; route list in `overview/README.md`.

- **Sign-in:** `/auth/login` → Keycloak (confidential `secretary-dashboard`, auth code + PKCE) → `/auth/callback`.
  - State, nonce and the PKCE verifier sit in a 10-minute HMAC-signed cookie.
  - The ID token is checked: issuer, audience, nonce, RS256 via JWKS.
  - `email` is required.
- **Keycloak URLs:** browser-facing ones use the public issuer; the token exchange and keys use `KEYCLOAK_INTERNAL_URL`.
- **Sessions:** server-side in `web_sessions` (token SHA-256 only); the cookie `sec_session` is HttpOnly, SameSite=Lax, and Secure on https.
  - A new token on every sign-in, so a planted cookie is never adopted.
  - Logout deletes the row and returns Keycloak's end-session URL.
- **Mutations** need `X-CSRF-Token` (from `GET /api/me`) and, when present, a same-origin `Origin`. Unknown body fields → 400.
- **Errors** are `{"error": code}`: 401 `signed_out`, 403 `bad_csrf` / `bad_origin`, 404 `not_found` (also for another user's ids), 400 `invalid_*`.
- **Activity** returns raw events of a local day (`day` + IANA `tz`), at most 5,000, with no totals: time is the engine's.
- **Projects and rules (D17):** projects are created in the personal workspace; rules are personal (`project_mappings.user_id` = the user), shared team rules come with Phase 3.
  - Rule semantics live in `shared/src/rules.rs` (used by the engine); the API has a Python copy for the preview. Both are tested against `shared/fixtures/rule-match-v1.json`: change the semantics only there and in both copies.
  - The preview (a POST, so it needs the CSRF token) counts events, not time, and reports conflicts with other projects' rules.
  - Rule and project changes queue `processing_jobs` (`rules_changed`) for the last 30 local days, one queued job per day.

## Contract Rules

- Request/response types for the device API and the event envelope come from `shared/`; TS types are generated.
- `user_id` and `device_id` are **never** accepted from payloads — derived from the token/session (`sub` → user, `sid` → device).
- Keycloak authenticates only. Authorization (workspaces, roles, visibility) lives in Postgres RLS; Keycloak roles/groups are never used for it.
- Batch ingest is idempotent on `event_id`; responses list accepted / duplicate / rejected IDs.
- Agent config uses ETag; agent polls and applies changes within ~5 min.
- Extensions never talk to the cloud directly — everything goes through the local agent.
- Overview API exposes engine results read-only; mutations write user inputs and enqueue jobs.
- Team endpoints (Phase 3) can return only `submissions` and aggregates — never another user's events, evidence, or sessions.
