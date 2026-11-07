# Domain — Tenancy, Workspaces & Teams

Status: planned. Phase 1 is single-user but the tenant model exists from the first migration. Source: blueprint §5.2, §5.5, §7, §14.1, §14.3.

## Model

- **User** — a person, identified by a Keycloak account (`sub`); GitHub/Google/SSO logins are linked inside Keycloak.
- **Workspace** — tenant boundary. Each user gets a **personal workspace** on sign-up; **team workspaces** (Phase 3) group members around shared projects.
- **Membership** — user ↔ workspace with a role: Owner, Admin, Member, Viewer.
- **Device** — belongs to exactly one user.
- **Project** — lives in one workspace. Personal projects never appear in a team.
- **Project link** — a member's mapping from their local repos/paths to a shared project (matched by normalized Git remote URL).
- **Submission** — a member's approved totals per shared project and period; the only per-user time data a team sees.

## Rules

- **Private until submitted.** Teammates and admins never see another member's raw events, evidence, sessions, devices, or unsubmitted time.
- Admins manage members, shared projects, shared rules, policies, and approvals — they cannot view or edit a member's raw activity, and there is no "view as" for raw data.
- Personal rules override shared rules in correlation.
- Submissions lock; changes create revisions. Recalculation that changes a submitted total asks the member to resubmit.
- Workspace policies (AI, retention, enabled collectors) combine with user settings — the stricter wins.
- Tenant identity always comes from the session/token, never from request data.
- Every tenant table and endpoint has an isolation test with two users / two workspaces.
- Workplace use may fall under employee-monitoring law; the product's transparency and disclosure screens exist for that reason — do not add features that weaken them.
