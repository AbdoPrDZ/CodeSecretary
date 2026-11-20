# Progress — Overview (`overview/`)

> **Plan:** [OVERVIEW.md](../plans/OVERVIEW.md) · **Overall:** [GENERAL.progress.md](GENERAL.progress.md)
>
> Tick only when acceptance criteria are met. Format: `- [x] ID — task · YYYY-MM-DD · note`. In progress: `· 🚧`. Blocked: `· ⛔ reason`.

## Phase 1

### M0
- [ ] OV-1 — API skeleton + web scaffold (Vite, router, query, UI kit)
  - Acceptance: both boot in Docker Compose

### M1
- [ ] OV-4 — Devices list/rename/revoke (revoke ends the Keycloak offline session)
  - Acceptance: revoked device rejected by warehouse

### M2
- [ ] OV-14 — Tray support endpoint (current project + today total)
  - Acceptance: tray shows live values

### M4
- [ ] OV-2 — Keycloak OIDC sign-in (GitHub/Google brokered), cookie sessions, CSRF, logout
  - Acceptance: sign in/out; session fixation & CSRF tests
- [ ] OV-3 — Personal workspace on first sign-in; RLS context per request
  - Acceptance: isolation tests pass
- [ ] OV-5 — Projects, mappings, exclusions, pause windows CRUD (+ config ETag bump)
  - Acceptance: agent picks up changes within 5 min
- [ ] OV-6 — Onboarding: detected repos → proposed projects; trigger backfill
  - Acceptance: new user reaches Today with data in < 10 min
- [ ] OV-7 — Today timeline + Week grid
  - Acceptance: matches engine output; multi-device shown
- [ ] OV-8 — Session detail with evidence
  - Acceptance: every session explains itself
- [ ] OV-9 — Corrections + job enqueue + recalculating state
  - [ ] Assign event(s)
  - [ ] Reassign session
  - [ ] Trim / extend
  - [ ] Split
  - [ ] Delete
  - [ ] Add manual time
  - [ ] Mapping change → re-enqueue affected days
  - Acceptance: correction visible after recalculation; survives algorithm re-run
- [ ] OV-10 — Inbox with bulk assign
  - Acceptance: unassigned + below-threshold items listed
- [ ] OV-11 — Reports + CSV export
  - Acceptance: CSV totals equal dashboard totals
- [ ] OV-12 — Privacy: data view, export, delete
  - Acceptance: delete removes data everywhere
- [ ] OV-13 — Settings: time zone (re-enqueue on change)
  - Acceptance: day boundaries correct after change

### M5
- [ ] OV-15 — Accessibility + responsive layout
  - Acceptance: keyboard navigable; usable at 360 px
- [ ] OV-16 — Playwright E2E: onboarding, review, correction, export
  - Acceptance: green in CI

### Security checklist
- [ ] HTTP-only, Secure, SameSite cookies; CSRF on mutations
- [ ] RLS context from session on every request, never from body
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
