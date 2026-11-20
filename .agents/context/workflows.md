# Workflows

Status: device login and the agent → warehouse upload path implemented (M1 demo); the rest planned. Source: blueprint §5, §9.1, §11.1; service plans; `agent/daemon/src/`, `warehouse/src/secretary_warehouse/`.

## Event Lifecycle

1. Collector produces an event (UUIDv7 ID).
2. Agent pipeline: validate → apply exclusions/pause → **redact** → write to SQLite buffer.
3. Uploader sends gzip batches (60 s or 500 events) with a Keycloak access token and clock offset; retries with backoff when offline.
4. Warehouse: authenticate → validate → adjust timestamps by clock offset → archive raw batch → insert (dedupe on `event_id`) → enqueue `(user, local day)` jobs (debounced ~2 min).
5. Engine claims job → loads day context → correlates → calculates → writes derived rows in one transaction.
6. Overview shows sessions; agent tray shows current project / today total.

Implemented so far:

- **Agent side:** the step-2 pipeline exists without the redact step, which waits for SH-5. So far only the presence collector uses it.
  - **Exclusions:** an event touching an excluded path or app is dropped whole.
  - **Pause windows:** device local time. Point events inside a window are dropped; intervals are cut at the window edges. `agent.pause` / `agent.resume` (`exclusion_window`) mark the edges.
  - **Config:** cached in `agent_config.json`, so it applies from the first event after a restart and while signed out or offline. It is refetched every 5 min with the `ETag`.
- **Warehouse side:** steps 4's authenticate, validate, adjust and insert are done. The raw archive (WH-6) and job enqueue (WH-7) are not.
- **Buffer rule:** buffered events leave the agent only when the warehouse acknowledges them, as accepted, duplicate or rejected. A lost response therefore costs a resend, never a duplicate.

## Device Login (Keycloak device authorization grant, D14)

Agent reads the issuer from warehouse `GET /v1/auth/config` → requests a device code from Keycloak → user signs in on Keycloak's device page, confirms the code and grants access (consent screen) → agent polls Keycloak and receives an access token and an offline refresh token → refresh token stored in the OS keychain → agent calls `POST /v1/devices` to register this Keycloak session as a device. Access tokens are short-lived and refreshed silently. Revoking a device in Overview marks it revoked and ends its Keycloak session. The warehouse rejects it within its 60 s lookup cache, and the agent asks the user to sign in again.

Status: implemented (agent CLI `login` / `logout` / `status`, AG-2; warehouse WH-4). Agent behaviour:

- **Where things are kept:** the refresh token sits in the OS keychain, in an entry named per installation. The access token stays in memory and is refreshed 30 s before it expires. Keycloak rotates the refresh token on every refresh, and the new one replaces the old in the keychain. `state.json` holds the issuer, endpoints, device id and e-mail, never tokens.
- **Signing itself out:** when a refresh returns `invalid_grant`, or a 401 comes back again after a refresh (e.g. `device_revoked`), the agent clears its keychain entry and account and logs a hint to run `login`. It skips the clearing if a newer `login` has already replaced the token.
- **While signed out:** collection continues into the buffer. The daemon re-checks the keychain every 30 s, so a `login` in another terminal resumes uploads without a restart.

## User Sign-in

Overview redirects to Keycloak (GitHub/Google brokered there) → auth code + PKCE → server-side cookie session. The first sign-in (dashboard or agent) maps the Keycloak `sub` to a new user and personal workspace.

## Recalculation

Triggered by new events, rule/mapping/override/adjustment/time-zone changes, or an algorithm version bump. Always a full recompute of the affected (user, local day); idempotent. Overview shows a "recalculating" state until the job finishes.

## Corrections

User edits in Overview are stored as inputs (user assignments, `time_adjustments`, mappings), never as edits to derived sessions. Engine reapplies them on every recalculation. Repeated corrections produce rule proposals (Phase 2).

## Team Submission (Phase 3)

Member reviews shared-project totals for a period → submits → submission becomes visible to the team and locks. Admin approves or requests changes. If recalculation later changes a submitted total, the member is asked to resubmit (creates a revision).

## Privacy Delete / Export

User can export all their raw events (NDJSON) and delete by event, day, device, project, or all — deletion covers Postgres and the raw archive, then derived data is recalculated.
