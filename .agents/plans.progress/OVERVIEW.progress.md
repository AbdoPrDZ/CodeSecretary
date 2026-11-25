# Progress — Overview (`overview/`)

> **Plan:** [OVERVIEW.md](../plans/OVERVIEW.md) · **Overall:** [GENERAL.progress.md](GENERAL.progress.md)
>
> Tick only when acceptance criteria are met. Format: `- [x] ID — task · YYYY-MM-DD · note`. In progress: `· 🚧`. Blocked: `· ⛔ reason`.

## Phase 1

### M0
- [x] OV-1 — API skeleton + web scaffold (Vite, router, query, UI kit) · 2026-11-20 · `overview/api` (FastAPI on `secretary-orm`, imports the warehouse schema) + `overview/web` (React 19, Vite 7, TanStack Router/Query, Tailwind 4; pnpm workspace); one image (`overview/Dockerfile`) serves API + built app; Compose service `overview` on :8090 boots healthy; CI jobs added
  - Acceptance: both boot in Docker Compose

### M1
- [ ] OV-4 — Devices list/rename/revoke (revoke ends the Keycloak offline session) · 🚧 in progress · 2026-11-20 · list (last seen, version, queue, online/revoked), rename, revoke; test: revoked from the dashboard → the warehouse answers 401 `device_revoked` (acceptance met); left: ending the Keycloak offline session on revoke (needs an admin service account)
  - Acceptance: revoked device rejected by warehouse

### M2
- [ ] OV-14 — Tray support endpoint (current project + today total)
  - Acceptance: tray shows live values

### M4
- [ ] OV-2 — Keycloak OIDC sign-in (GitHub/Google brokered), cookie sessions, CSRF, logout · 🚧 in progress · 2026-11-20 · auth code + PKCE (`secretary-dashboard`), HMAC-signed login-state cookie, server-side sessions in `web_sessions` (token hash only, RLS), HttpOnly/SameSite=Lax/Secure-on-https cookie, CSRF header + Origin check on mutations, RP logout; tests: PKCE, state/code/nonce rejection, CSRF/Origin, expiry, logout, session fixation; E2E against Compose Keycloak; left: GitHub/Google brokering (needs OAuth apps)
  - Acceptance: sign in/out; session fixation & CSRF tests
- [x] OV-3 — Personal workspace on first sign-in; RLS context per request · 2026-11-20 · `app_resolve_identity()` on callback provisions user + personal workspace; every request runs in `user_transaction(session.user_id)`; isolation test: user B sees none of A's devices, events, exclusions and cannot rename/revoke/delete them (404)
  - Acceptance: isolation tests pass
- [ ] OV-5 — Projects, mappings, exclusions, pause windows CRUD (+ config ETag bump) · 🚧 in progress · 2026-11-20 · exclusions (folder/app/website) and pause windows: list/add/remove with validation; the agent config (ETag = body hash) changes with them, tested via `agent_config.build`. 2026-11-21: projects (create, edit, archive, delete) and personal rules with normalization, match preview (event counts, values, conflicts) and detected-repository suggestions; rule changes queue recalculation of 30 days; 5 API tests + shared fixture tests (Rust and Python). Left: a manual UI pass in the browser
  - Acceptance: agent picks up changes within 5 min
- [ ] OV-6 — Onboarding: create projects and rules (D17); detected repos offered as optional suggestions; trigger backfill
  - Acceptance: new user reaches Today with data in < 10 min
- [ ] OV-7 — Today timeline + Week grid · 🚧 in progress · 2026-11-25 · first slice: Time page (default route) with day / week / month totals per project from `GET /api/time` (adds up the engine's sessions), per-day stacked bars, the day's session timeline + list (`GET /api/sessions`), recalculating / failed state, time-zone prompt; 4 API tests (totals, privacy, time zone, compression); left: multi-device lanes, session detail (OV-8)
  - Acceptance: matches engine output; multi-device shown
- [ ] OV-8 — Session detail with evidence · 🚧 in progress · 2026-11-25 · `GET /api/sessions/{id}`: evidence events (≤ 500) with device, assignment method, rule and override flag, method counts, the matching rules; session drawer on the Time page ("why this session"); API test incl. another user → 404; left: a manual UI pass in the browser
  - Acceptance: every session explains itself
- [ ] OV-9 — Corrections + job enqueue + recalculating state · 🚧 in progress · 2026-11-25 · `POST /api/assign` (events or sessions' evidence → `assignment_overrides`, "not project work", clear), `GET/POST/DELETE /api/adjustments` (reassign, remove, add; split at local midnight; undo = `deleted_at`); every change queues its days to run now (`enqueue_days` pulls a queued job forward); drawer actions + corrections card with undo + "Add time"; the engine applies them (EN-10, survives recalculation, DB test); 5 API tests; left: a manual UI pass in the browser
  - [x] Assign event(s) · 2026-11-25 · overrides; whole sessions via their evidence
  - [x] Reassign session · 2026-11-25 · `reassign` over the session range, from its project
  - [x] Trim / extend · 2026-11-25 · trim = `remove` of a part; extend = "Add time" next to it
  - [x] Split · 2026-11-25 · `reassign` of a part to another project
  - [x] Delete · 2026-11-25 · `remove` over the session range, from its project
  - [x] Add manual time · 2026-11-25 · `add` → manual session (no evidence)
  - [x] Mapping change → re-enqueue affected days · 2026-11-21 · OV-5 (last 30 days); since 2026-11-25 queued days are pulled forward
  - Acceptance: correction visible after recalculation; survives algorithm re-run
- [ ] OV-10 — Inbox with bulk assign · 🚧 in progress · 2026-11-25 · `GET /api/inbox`: unassigned sessions (≤ 1,000) grouped by their most common folder, else app, with titles; Inbox page: assign a group's sessions or "Always" (adds a path/app rule → 30 days recalculated); API test incl. isolation; left: a manual UI pass in the browser; below-threshold items do not exist yet (lowest automatic confidence is temporal 0.7 = the threshold)
  - Acceptance: unassigned + below-threshold items listed
- [ ] OV-11 — Reports + CSV export
  - Acceptance: CSV totals equal dashboard totals
- [ ] OV-12 — Privacy: data view, export, delete · 🚧 in progress · 2026-11-20 · data view: Activity page (per-device day timeline of raw events in the browser's zone + event list with metadata; no totals); left: export, delete
  - Acceptance: delete removes data everywhere
- [x] OV-13 — Settings: time zone (re-enqueue on change) · 2026-11-25 · `PUT /api/me/time_zone` (validated IANA name, CSRF) re-queues the last 30 days; the Time page offers the browser's zone when it differs; test: day bounds follow the new zone. No full settings page yet
  - Acceptance: day boundaries correct after change

### M5
- [ ] OV-15 — Accessibility + responsive layout
  - Acceptance: keyboard navigable; usable at 360 px
- [ ] OV-16 — Playwright E2E: onboarding, review, correction, export
  - Acceptance: green in CI

### Security checklist
- [x] HTTP-only, Secure, SameSite cookies; CSRF on mutations · 2026-11-20 · Secure when PUBLIC_URL is https; tests for CSRF + Origin
- [x] RLS context from session on every request, never from body · 2026-11-20 · bodies with extra fields (e.g. `user_id`) are rejected; isolation test
- [ ] No route returns another user's events, evidence, or sessions
- [ ] CSP, HSTS, no inline scripts
- [ ] Rate limiting on auth and export endpoints

## Phase 2
- [ ] OV-20 — AI settings, provider config, budget, request log viewer
- [ ] OV-21 — AI summaries with evidence links
- [ ] OV-22 — Rule proposals UI
- [ ] OV-23 — Collector/device health, coverage gaps on timeline
- [ ] OV-24 — Android companion endpoints + package-name mappings (D16)

## Phase 3
- [ ] OV-30 — Invite-only sign-up (friends)
- [ ] OV-31 — Team workspaces, invitations, roles
- [ ] OV-32 — Shared projects + shared rules; project links by remote URL
- [ ] OV-33 — Submission flow (submit, lock, revisions, resubmission notices)
- [ ] OV-34 — Approvals
- [ ] OV-35 — Team reports (submitted totals only, enforced by RLS)
- [ ] OV-36 — Workspace settings: AI policy, retention, visibility
- [ ] OV-37 — Audit log
- [ ] OV-38 — Join-team privacy disclosure
- [ ] OV-39 — Self-hosting docs + Compose file
- [ ] Security: audit log for admin actions

## Phase 4
- [ ] OV-40 — Hourly rates, billable flags, waiting-time policy, rounding
- [ ] OV-41 — Budgets: estimated vs. actual
- [ ] OV-42 — Excel export
- [ ] OV-43 — Integrations: Jira, Harvest, Toggl, Clockify
- [ ] OV-44 — Company SSO via Keycloak identity providers

## Notes
- 2026-11-25: Corrections deviate from the plan's §5 storage on purpose, following the schema (`database.md`): event assignments are `assignment_overrides` (not `event_assignments` rows with `method = user`; the engine turns them into those), and the plan's trim / split / delete kinds are the schema's `remove` and `reassign` ranges with an optional from-project filter. Corrections crossing a local midnight are stored as one row per day.
- 2026-11-25: Dashboard speed (user report "loading is too slow"): responses were sent uncompressed (374 kB JS + 63 kB CSS + up to 5,000 events of JSON); added gzip (`GZipMiddleware`, > 1 KB) and `encode zstd gzip` in the VPS Caddyfile. The session lookup was two transactions with an UPDATE on every request; now one transaction, `last_seen_at` written at most every 5 min. The web app now has a default `staleTime` of 30 s (no refetch of a whole day on every focus), and the Activity table renders 300 rows at a time. Not measured on the VPS yet.
- 2026-11-20: First dashboard slice, chosen by the user: sign-in, devices, raw activity and exclusions/pause windows, local first, then the VPS.
  - **OV-5:** exclusions and pause windows first; projects and rules added 2026-11-21.
  - **OV-12:** the data view only.
  - **Activity page:** raw events only, with no totals, because only the engine computes time.
- 2026-11-20:
  - **Sessions table:** `web_sessions` is part of the warehouse schema (D15 ownership), with an RLS policy and the SECURITY DEFINER lookup `app_web_session_user()`.
  - **Database login:** dev Compose reuses the `secretary_warehouse` login for the dashboard. A separate `secretary_overview` login belongs to the hardening work.
  - **Generated types:** the TypeScript contract is generated into `overview/web/src/generated/` as well (SH-9).
- 2026-11-21: Decision D17 (user): projects are created by the user and bound to evidence by rules; detected repositories are optional suggestions. OV-5 now includes a match preview for rules; OV-6 rescoped accordingly.
- 2026-11-21: OV-5 rules: one semantics in `shared/src/rules.rs` (for the engine) with a Python copy in `overview/api` for the preview, both checked by `shared/fixtures/rule-match-v1.json`. Defaults taken (no answer yet, easy to change): rule changes recalculate the last 30 days; unmatched activity goes to the review inbox (D17). Website rules match nothing until the browser extension (Phase 2) sends domains.
- 2026-11-21: Dashboard redesign (user request): design system in `overview/web/src/components/ui.tsx` (buttons, inputs, cards, badges, stats, dialogs, drawers) and inline SVG icons (`icons.tsx`, no icon package); sidebar shell with a mobile drawer; light/dark/system theme (class on `<html>`, choice in localStorage); self-hosted Inter (`@fontsource-variable/inter` 5.3.0, CSP allows only self). All four pages rebuilt. Checked by screenshots in headless Edge (light, dark, 390 px). Time is still not summed anywhere in the web app.
