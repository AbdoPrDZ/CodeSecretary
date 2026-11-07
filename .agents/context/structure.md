# Context Structure Specification

Rules for what lives in `.agents/context/`. This file changes only when the **organization** of the context changes.

Context is derived knowledge: **actual project > this context > blueprint/plans**. When code exists and disagrees with the context, trust the code and fix the context.

## Relationship to Other Project Documents

| Location | Role | Context must not duplicate it |
|---|---|---|
| `.agents/blueprints/IDEA-V4.md` | Product blueprint — full what/why | Link to sections instead of copying |
| `.agents/plans/*.md` | Task-level implementation plans | Never list tasks in context |
| `.agents/plans.progress/*.md` | Task status | Never track status in context |
| Source code | Ground truth once written | No inventories of files, functions, columns, endpoints |

Context holds the **stable, cross-cutting knowledge and rules** an agent needs before touching code.

## Files

| File | Purpose | Belongs | Does not belong |
|---|---|---|---|
| `README.md` | Entry point and navigation | What the context is, reading order, which file to read per task type | Any project knowledge beyond a 3-line summary |
| `config.md` | Maintenance settings | Update mode, detail level, scope, template | Anything else |
| `structure.md` | This specification | Files, responsibilities, creation rules, levels | Project knowledge |
| `architecture.md` | System shape | Services, responsibilities, data flow, ownership boundaries, deployment shape, tech stack | Per-service task lists, endpoint details, schema columns |
| `conventions.md` | How code is written here | Repo layout, languages/tools, testing rules, versioning, logging rules, git rules | Generic framework docs |
| `database.md` | Data model concepts | Raw vs. derived vs. user data, write ownership, tenancy/RLS rules, migration rules, partitioning | Column lists, full table inventories |
| `api.md` | API surfaces and contracts | Which APIs exist, who calls them, auth model per surface, contract rules | Per-endpoint request/response specs (those live in code / OpenAPI) |
| `workflows.md` | End-to-end flows | Event lifecycle, device login, recalculation, corrections, submissions | UI screen details |
| `domains/README.md` | Domain index | List of domain files, one line each | Domain content |
| `domains/time-calculation.md` | Core domain | Correlation and calculation rules, invariants, determinism, versioning | Default numeric values beyond pointers to Appendix A |
| `domains/privacy.md` | Privacy & security domain | Redaction rules, visibility model, threat-driven rules agents must not break | Generic security advice |
| `domains/tenancy.md` | Users, workspaces, teams | Tenant model, roles, isolation rules, private-until-submitted | Auth provider configuration details |

## Creation Rules

- `decisions.md` — create only at detail level `detailed`, or when decisions stop fitting in the blueprint decision log.
- `infrastructure.md` — create when a real deployment (staging/production) exists.
- `integrations.md` — create when the first external integration (AI provider, Jira, Harvest…) is implemented.
- `frontend.md` — create when `overview/web` has established UI conventions worth recording.
- New domain file — create when a domain has its own business rules that do not fit an existing domain file (e.g., `billing.md` in Phase 4, `ai.md` when the AI layer is implemented).
- Never create one file per service, module, or table.

## Structure Per Detail Level

```text
minimal:   README.md, config.md, structure.md, architecture.md, conventions.md
standard:  + database.md, api.md, workflows.md, domains/ (time-calculation, privacy, tenancy)
detailed:  + decisions.md, infrastructure.md, integrations.md, more domains
```

Current level: **standard** (see `config.md`).

## Status Markers

Until code exists, describe the **planned** design and mark it with `Status: planned`. Once a part is implemented, verify against the code, remove the marker for that part, and correct anything that differs.
