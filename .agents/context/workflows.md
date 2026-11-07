# Workflows

Status: planned. Source: blueprint §5, §9.1, §11.1; service plans.

## Event Lifecycle

1. Collector produces an event (UUIDv7 ID).
2. Agent pipeline: validate → apply exclusions/pause → **redact** → write to SQLite buffer.
3. Uploader sends gzip batches (60 s or 500 events) with a Keycloak access token and clock offset; retries with backoff when offline.
4. Warehouse: authenticate → validate → adjust timestamps by clock offset → archive raw batch → insert (dedupe on `event_id`) → enqueue `(user, local day)` jobs (debounced ~2 min).
5. Engine claims job → loads day context → correlates → calculates → writes derived rows in one transaction.
6. Overview shows sessions; agent tray shows current project / today total.

## Device Login (Keycloak device authorization grant, D14)

Agent reads the issuer from warehouse `GET /v1/auth/config` → requests a device code from Keycloak → user signs in on Keycloak's device page and confirms the code → agent polls Keycloak and receives an access token and an offline refresh token → refresh token stored in the OS keychain → agent calls `POST /v1/devices` to register this Keycloak session as a device. Access tokens are short-lived and refreshed silently. Revoking a device in Overview marks it revoked and ends its Keycloak session; the warehouse rejects it at once and the agent asks the user to sign in again.

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
