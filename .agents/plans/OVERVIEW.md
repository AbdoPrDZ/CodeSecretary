# Service Plan — Overview (`overview/`)

> **Blueprint:** [IDEA-V4.md](../blueprints/IDEA-V4.md) §5, §13, §14.1, §14.3 · **General plan:** [GENERAL.md](GENERAL.md)

## 1. Purpose

The **user-facing** part of the platform: authentication, workspaces, projects and mappings, review and corrections, reports, and (Phase 3) teams and submissions.

**It never computes time.** It reads Engine results and writes user inputs; any input that affects calculation enqueues an Engine job.

## 2. Components

| Component | Folder | Technology |
|---|---|---|
| Dashboard API | `overview/api/` | Python, FastAPI, `secretary-orm` (imports the warehouse's models, D15) |
| Web app | `overview/web/` | React, TypeScript, Vite, TanStack Query, TanStack Router, a component library (e.g., shadcn/ui), a timeline/chart library |

## 3. API Structure

```
overview/api/src/secretary_overview/
├─ __main__.py
├─ auth/            # Keycloak OIDC client (auth code + PKCE), server-side sessions (HTTP-only secure cookies), CSRF
├─ rls.py           # per-request RLS context from session (user_transaction)
├─ workspaces/      # personal workspace on signup; teams in Phase 3
├─ devices/         # list, rename, revoke (also deletes the Keycloak offline session)
├─ projects/        # CRUD, project links, mappings, exclusions
├─ timeline/        # sessions, evidence for a day/range (read-only)
├─ inbox/           # unassigned / low-confidence events
├─ corrections/     # overrides, adjustments → enqueue jobs
├─ reports/         # aggregates, CSV export
├─ privacy/         # data view, export, delete (calls warehouse procedure)
├─ settings/        # time zone, AI mode, defaults overrides
└─ team/            # Phase 3: invites, roles, submissions, approvals, audit log
```

## 4. Web App Screens

| Screen | Phase | Contents |
|---|---|---|
| Sign in | 1 | Redirect to Keycloak (GitHub / Google brokered) |
| Onboarding | 1 | Download agent, device approval, project roots, detected repos → projects, backfill, AI mode |
| **Device approval** | 1 | Hosted by Keycloak (`/realms/secretary/device`); Overview only links to it during onboarding |
| **Today** | 1 | Timeline (lane per project, across devices), totals, active vs. waiting |
| **Week** | 1 | Per-day per-project grid; totals |
| **Session detail** | 1 | Evidence list (why), trim / split / reassign / delete |
| **Inbox** | 1 | Unassigned + low-confidence activity, bulk assign |
| Projects | 1 | Projects, mappings, linked repos, colors |
| Devices | 1 | Last seen, version, sync status, revoke |
| Exclusions & pause windows | 1 | Directories, apps, domains, time windows |
| Privacy | 1 | Recent collected data, export, delete |
| Reports | 1 | Range + projects → totals, CSV |
| Settings | 1 | Time zone, account |
| AI | 2 | Mode, provider, budget, request log |
| Health | 2 | Collectors, coverage gaps |
| Rule proposals | 2 | Accept / reject |
| Team: members, roles, invites | 3 | |
| Team: shared projects & rules | 3 | |
| **Submit** | 3 | Pick period → review totals per shared project → submit |
| Team reports & approvals | 3 | Submitted totals only |
| Audit log | 3 | Admin actions |
| Billing: rates, budgets, rounding | 4 | |
| Integrations | 4 | Jira, Harvest, Toggl, Clockify |

## 5. Correction Semantics

| Action | Stored as | Effect |
|---|---|---|
| Assign event(s) to project | `event_assignments` with `method = user` | Enqueue (user, day) |
| Reassign session | `time_adjustments{kind: reassign, range, from, to}` | Enqueue |
| Trim / extend session | `time_adjustments{kind: trim, range, project}` | Enqueue |
| Split session | `time_adjustments{kind: split, at, project_a, project_b}` | Enqueue |
| Delete session | `time_adjustments{kind: delete, range, project}` | Enqueue |
| Add manual time | `time_adjustments{kind: add, range, project, note}` | Enqueue |
| Create/edit mapping | `project_mappings` | Enqueue affected days (last 30 by default, user can choose) |

The UI shows a "recalculating…" state until the job completes (poll job status or SSE).

## 6. Tasks

### Phase 1

| ID | Task | Milestone | Acceptance |
|---|---|---|---|
| OV-1 | API skeleton, config, health, tracing; web scaffold (Vite, router, query, UI kit) | M0 | Both boot in Docker Compose |
| OV-2 | Keycloak OIDC sign-in (GitHub/Google brokered), server-side cookie sessions, CSRF, logout | M4 | Sign in/out works; session fixation & CSRF tests |
| OV-3 | Personal workspace creation on first sign-in; RLS context per request | M4 | Isolation tests pass |
| OV-4 | Devices list/rename/revoke (revoke ends the Keycloak offline session) | M1 | Revoked device rejected by warehouse |
| OV-5 | Projects (created by the user, D17), mapping rules with a match preview (folders, remotes, domains, apps, titles), exclusions, pause windows CRUD (+ agent config ETag bump) | M4 | Agent picks up changes within 5 min; a rule shows how many recent events it matches before saving |
| OV-6 | Onboarding: create projects and rules (D17); detected repos offered as optional suggestions; trigger backfill | M4 | New user reaches Today view with data in < 10 min |
| OV-7 | Today timeline + Week grid (read sessions) | M4 | Matches engine output; multi-device shown |
| OV-8 | Session detail with evidence | M4 | Every session explains itself |
| OV-9 | Corrections (all actions in §5) + job enqueue + recalculating state | M4 | Correction visible after recalculation; survives algorithm re-run |
| OV-10 | Inbox with bulk assign | M4 | Unassigned + below-threshold items listed |
| OV-11 | Reports + CSV export | M4 | CSV totals equal dashboard totals |
| OV-12 | Privacy: data view, export, delete | M4 | Delete removes data everywhere (via warehouse procedure) |
| OV-13 | Settings: time zone (affects local-day processing → re-enqueue) | M4 | Day boundaries correct after change |
| OV-14 | Tray support endpoint: current project + today total for the agent | M2 | Tray shows live values |
| OV-15 | Accessibility + responsive layout | M5 | Keyboard navigable; usable at 360 px |
| OV-16 | E2E tests (Playwright) for onboarding, review, correction, export | M5 | Green in CI |

### Phase 2

| ID | Task |
|---|---|
| OV-20 | AI settings, provider config, budget, request log viewer |
| OV-21 | AI summaries display with evidence links |
| OV-22 | Rule proposals UI |
| OV-23 | Collector / device health, coverage gaps on timeline |
| OV-24 | Companion endpoints for the Android app (today total, review queue) and package-name mapping rules (D16) |

### Phase 3

| ID | Task |
|---|---|
| OV-30 | Invite-only sign-up (friends) |
| OV-31 | Team workspaces, invitations, roles (Owner, Admin, Member, Viewer) |
| OV-32 | Shared projects + shared rules; member project links by remote URL |
| OV-33 | Submission flow (submit, lock, revisions, resubmission notices) |
| OV-34 | Approvals (approve / request changes) |
| OV-35 | Team reports (submitted totals only — enforced by RLS, not UI) |
| OV-36 | Workspace settings: AI policy, retention, visibility of submitted totals |
| OV-37 | Audit log |
| OV-38 | Join-team privacy disclosure screen |
| OV-39 | Self-hosting docs + Compose file |

### Phase 4

| ID | Task |
|---|---|
| OV-40 | Hourly rates, billable flags, waiting-time policy, rounding |
| OV-41 | Budgets: estimated vs. actual |
| OV-42 | Excel export |
| OV-43 | Integrations: Jira, Harvest, Toggl, Clockify |
| OV-44 | Company SSO: per-workspace Keycloak identity providers (OIDC/SAML) |

## 7. Security Checklist

- [ ] HTTP-only, Secure, SameSite cookies; CSRF tokens on mutations.
- [ ] RLS context set from session on every request; never from request body.
- [ ] Team endpoints can only read `submissions` and aggregates — no route returns another user's events, evidence, or sessions.
- [ ] CSP, HSTS, no inline scripts.
- [ ] Rate limiting on auth and export endpoints.
- [ ] Audit log for admin actions (Phase 3).

## 8. Testing

- API integration tests (testcontainers) incl. isolation for every endpoint.
- Web: Vitest component tests; Playwright E2E against Docker Compose with fixture data.
- Contract: generated TS types from `shared/` and API (e.g., OpenAPI via `utoipa` → TS client).

## 9. Risks

| Risk | Mitigation |
|---|---|
| Timeline UI complexity | Start with simple lanes; virtualize; iterate after M5 |
| Users confused by recalculation delay | Clear "recalculating" state; fast engine (< 500 ms/day) |
| Team visibility leaks via UI bugs | Visibility enforced by RLS and API, tested independently of UI |
