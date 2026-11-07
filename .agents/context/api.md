# APIs

Status: planned. Source: blueprint Appendix C; [WAREHOUSE plan](../plans/WAREHOUSE.md) §4; [AGENT plan](../plans/AGENT.md) §3.4; [OVERVIEW plan](../plans/OVERVIEW.md) §3.

## Surfaces

| Surface | Host | Callers | Auth |
|---|---|---|---|
| **Local agent API** (`/local/v1/*`) | agent, `127.0.0.1` random port | VS Code ext, shell hooks | Per-collector bearer token from a user-only file; requests with an `Origin` header rejected |
| **Native Messaging** (Phase 2) | agent | Browser extension | Browser-managed host registration |
| **Identity** (OIDC endpoints) | Keycloak, realm `secretary` | agent, overview | Agent: device authorization grant (`secretary-agent`, public client, offline token). Dashboard: auth code + PKCE |
| **Device API** (`/v1/auth/config`, `/v1/devices`, `/v1/events:batch`, `/v1/agent/*`) | warehouse | agent only | Keycloak access token (`aud` `secretary-warehouse`, `azp` `secretary-agent`); its `sid` must map to a registered, unrevoked device |
| **User API** | overview/api | overview/web | Keycloak OIDC sign-in → HTTP-only secure cookie session + CSRF |

## Contract Rules

- Request/response types for the device API and the event envelope come from `shared/`; TS types are generated.
- `user_id` and `device_id` are **never** accepted from payloads — derived from the token/session (`sub` → user, `sid` → device).
- Keycloak authenticates only. Authorization (workspaces, roles, visibility) lives in Postgres RLS; Keycloak roles/groups are never used for it.
- Batch ingest is idempotent on `event_id`; responses list accepted / duplicate / rejected IDs.
- Agent config uses ETag; agent polls and applies changes within ~5 min.
- Extensions never talk to the cloud directly — everything goes through the local agent.
- Overview API exposes engine results read-only; mutations write user inputs and enqueue jobs.
- Team endpoints (Phase 3) can return only `submissions` and aggregates — never another user's events, evidence, or sessions.
