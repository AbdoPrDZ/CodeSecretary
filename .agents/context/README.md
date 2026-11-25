# Project Context — CodeSecretary

AI-readable memory for this project. Read this before substantial work; keep it accurate as the project changes (update mode in [config.md](config.md)).

**CodeSecretary** — a local agent collects developer activity evidence; a cloud platform turns it into explainable, correctable time per project for individuals and (later) teams.

**Project stage:** early implementation (M1). The `shared/` contract crate, the agent daemon (Keycloak login, presence, buffer, uploader, heartbeat), the `secretary-orm` package and the Python warehouse (ORM-owned schema with RLS, device API: auth, registration, ingest, heartbeat) exist; the agent → warehouse path works end to end in Docker Compose. The overview dashboard has its first slice (Keycloak sign-in, devices, raw activity timeline, exclusions and pause windows, projects and rules, time per project); a single-VPS deployment exists (`deploy/vps/`). The engine v1 (job queue, correlation, sessions) runs in Compose. Files marked `Status: planned` describe intended design, not code.

## Reading Order

```
README.md → config.md → structure.md → files for your task → source code
```

## What to Read per Task

| Task | Read |
|---|---|
| Any task | [architecture.md](architecture.md), [conventions.md](conventions.md) |
| Agent / collectors | architecture, [domains/privacy.md](domains/privacy.md), [workflows.md](workflows.md) |
| Ingest / device API | [api.md](api.md), [workflows.md](workflows.md), [domains/privacy.md](domains/privacy.md) |
| Database / schema | [database.md](database.md), [domains/tenancy.md](domains/tenancy.md) |
| Engine / time calculation | [domains/time-calculation.md](domains/time-calculation.md), [database.md](database.md) |
| Dashboard / API for users | [api.md](api.md), [domains/tenancy.md](domains/tenancy.md), [workflows.md](workflows.md) |
| Teams, roles, sharing | [domains/tenancy.md](domains/tenancy.md), [domains/privacy.md](domains/privacy.md) |

## Files

- [config.md](config.md) — maintenance settings
- [structure.md](structure.md) — what each context file is for
- [architecture.md](architecture.md) — services, data flow, ownership, stack
- [conventions.md](conventions.md) — repo layout, tooling, testing, git
- [database.md](database.md) — data model concepts and rules
- [api.md](api.md) — API surfaces and contracts
- [workflows.md](workflows.md) — end-to-end flows
- [domains/](domains/README.md) — time calculation, privacy, tenancy

## Related Documents (not context)

- Blueprint: [../blueprints/IDEA-V4.md](../blueprints/IDEA-V4.md) (V1–V3 are historical)
- Plans: [../plans/GENERAL.md](../plans/GENERAL.md)
- Progress: [../plans.progress/GENERAL.progress.md](../plans.progress/GENERAL.progress.md)
