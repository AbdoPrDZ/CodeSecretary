# APIs

Status: device API partly implemented (auth config, devices, batch ingest, heartbeat; agent config is WH-9); other surfaces planned. Source: `warehouse/src/secretary_warehouse/{http,auth,ingest}.py`, `shared/src/api.rs`; blueprint Appendix C; [WAREHOUSE plan](../plans/WAREHOUSE.md) §4; [AGENT plan](../plans/AGENT.md) §3.4; [OVERVIEW plan](../plans/OVERVIEW.md) §3.

## Surfaces

| Surface | Host | Callers | Auth |
|---|---|---|---|
| **Local agent API** (`/local/v1/*`) | agent, `127.0.0.1` random port | VS Code ext, shell hooks | Per-collector bearer token from a user-only file; requests with an `Origin` header rejected |
| **Native Messaging** (Phase 2) | agent | Browser extension | Browser-managed host registration |
| **Identity** (OIDC endpoints) | Keycloak, realm `secretary` | agent, overview | Agent: device authorization grant (`secretary-agent`, public client, offline token). Dashboard: auth code + PKCE |
| **Device API** (`/v1/auth/config`, `/v1/devices`, `/v1/events:batch`, `/v1/agent/*`) | warehouse | agent only | Keycloak access token (`aud` `secretary-warehouse`, `azp` `secretary-agent`); its `sid` must map to a registered, unrevoked device |
| **User API** | overview/api | overview/web | Keycloak OIDC sign-in → HTTP-only secure cookie session + CSRF |

## Device API Behaviour

Status: implemented (WH-4, WH-5, WH-8). Source: `warehouse/src/secretary_warehouse/`.

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
- **Mirrored validation:** the warehouse mirrors the `shared/` envelope and validation by hand in `ingest.py` until SH-9 generates a JSON Schema. Change both together.

## Contract Rules

- Request/response types for the device API and the event envelope come from `shared/`; TS types are generated.
- `user_id` and `device_id` are **never** accepted from payloads — derived from the token/session (`sub` → user, `sid` → device).
- Keycloak authenticates only. Authorization (workspaces, roles, visibility) lives in Postgres RLS; Keycloak roles/groups are never used for it.
- Batch ingest is idempotent on `event_id`; responses list accepted / duplicate / rejected IDs.
- Agent config uses ETag; agent polls and applies changes within ~5 min.
- Extensions never talk to the cloud directly — everything goes through the local agent.
- Overview API exposes engine results read-only; mutations write user inputs and enqueue jobs.
- Team endpoints (Phase 3) can return only `submissions` and aggregates — never another user's events, evidence, or sessions.
