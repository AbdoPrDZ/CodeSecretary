# Service Plan — Shared (`shared/`)

> **Blueprint:** [IDEA-V4.md](../blueprints/IDEA-V4.md) §6.1, §9.2, §14.2, Appendix A–B · **General plan:** [GENERAL.md](GENERAL.md)

## 1. Purpose

A Rust crate (`secretary-shared`) that is the **single source of truth** for everything more than one service must agree on. Compiled into the agent and engine. TypeScript types are generated from it for the VS Code extension, browser extension, and web app, and a JSON Schema of the event envelope and device API for the Python warehouse and overview (D15).

## 2. Scope

**In scope**

- Event envelope and per-type metadata structs.
- Event type catalog (Appendix B) as an enum.
- Validation (required fields, sizes, timestamp sanity, per-type rules).
- Redaction engine and default rule set.
- Agent config types (exclusions, project roots, pause windows, redaction overrides).
- Default configuration values (Appendix A) as typed constants.
- Batch upload request/response types for the device API.
- Common IDs (`UserId`, `DeviceId`, `ProjectId`, `WorkspaceId`, `EventId`) as newtypes.

**Out of scope**

- Database access, HTTP servers, OS APIs — those live in the services.

## 3. Module Structure

```
shared/
├─ Cargo.toml
└─ src/
   ├─ lib.rs
   ├─ ids.rs            # newtype IDs, UUIDv7 generation
   ├─ event/
   │  ├─ mod.rs         # Event envelope, SCHEMA_VERSION
   │  ├─ types.rs       # EventType enum
   │  └─ metadata.rs    # per-type metadata structs (tagged enum)
   ├─ validate.rs       # validation rules + errors
   ├─ redact/
   │  ├─ mod.rs         # Redactor, RedactionRule
   │  ├─ patterns.rs    # default secret patterns
   │  └─ url.rs         # URL stripping (query, fragment, userinfo)
   ├─ config.rs         # AgentConfig, Exclusions, defaults (Appendix A)
   ├─ api.rs            # BatchRequest, BatchResponse, Heartbeat, device-flow types
   └─ ts.rs             # ts-rs export entry point
```

## 4. Key Designs

### 4.1 Event Envelope

```rust
pub struct Event {
    pub event_id: EventId,                 // UUIDv7
    pub schema_version: u16,
    pub timestamp_start: DateTime<Utc>,
    pub timestamp_end: Option<DateTime<Utc>>,
    pub source: Source,                    // { name, version }
    pub project_hint: Option<ProjectId>,
    pub repository: Option<RepositoryRef>, // { root_path, remote_url_normalized }
    pub working_directory: Option<String>,
    pub metadata: Metadata,                // #[serde(tag = "event_type")]
}
```

`user_id` and `device_id` are **not** in the client envelope — Warehouse sets them from the access token (`sub` → user, `sid` → device).

### 4.2 Redaction

- `Redactor::apply(&mut Event)` runs every rule appropriate for the event type.
- Default rules:
  - Terminal: keep command **name** only (first token, or first two for known multi-word tools like `git push`, `npm run`, `cargo test`); drop args unless the project opts in, then mask secret patterns.
  - URL: drop query, fragment, userinfo; keep scheme + host + path; path segments that look like tokens (long hex/base64) replaced with `:id`.
  - Window titles: mask emails, secret patterns; truncate to 200 chars.
  - Secret patterns: AWS keys, GitHub/GitLab tokens, Slack tokens, JWTs, `Bearer …`, `KEY=value` where key matches `*KEY|*TOKEN|*SECRET|*PASSWORD`, connection strings with credentials, private key headers.
- Rules are versioned; `redaction_version` stored in event metadata for audit.

### 4.3 Remote URL Normalization

`git@github.com:acme/api.git`, `https://github.com/acme/api`, and `ssh://git@github.com/acme/api.git` → `github.com/acme/api`. Used for matching repos to shared projects.

## 5. Tasks

### Phase 1

| ID | Task | Milestone | Acceptance |
|---|---|---|---|
| SH-1 | Create crate, wire into workspace, CI | M0 | Builds in CI on 3 OSes |
| SH-2 | IDs + UUIDv7 | M1 | Time-sortable IDs; round-trip serde |
| SH-3 | Event envelope + Phase 1 event types (`presence.*`, `window.focus`, `editor.*`, `git.*`, `agent.*`) | M1 | Serde round-trip tests for every type |
| SH-4 | Validation | M1 | Rejects: missing fields, end < start, future > 5 min skew, metadata > 8 KB, unknown type |
| SH-5 | Redaction engine + default rules | M1 | Redaction corpus test (≥ 100 secret samples) — zero survive |
| SH-6 | URL stripping and remote URL normalization | M1 | Table-driven tests incl. edge cases |
| SH-7 | Device API types (auth config, register, batch, heartbeat, config) | M1 | Used by agent and warehouse without duplication |
| SH-8 | AgentConfig + Appendix A defaults | M1 | Defaults match blueprint |
| SH-9 | Type generation: TS (`ts-rs`) → `agent/vscode`, `overview/web`; JSON Schema (`schemars`) → warehouse, overview API | M1 | CI fails if generated types or schemas are stale |

### Phase 2

| ID | Task |
|---|---|
| SH-20 | Event types: `terminal.command`, `browser.tab_focus`, `fs.change` |
| SH-21 | Terminal argument opt-in redaction path |
| SH-22 | `schema_version` 2 migration helpers (accept N and N-1) |
| SH-23 | Kotlin types + validation for the Android app from the SH-9 JSON Schema; CI fails if stale (D16) |

### Phase 3

| ID | Task |
|---|---|
| SH-30 | Team config types (shared project link hints, workspace policies pushed to agent) |

## 6. Testing

- Serde round-trip for every event type.
- Property tests (`proptest`) for redaction: random strings with embedded secrets → secrets never survive.
- Golden files for URL and remote normalization.
- Redaction corpus is a versioned fixture used by agent tests too.

## 7. Risks

| Risk | Mitigation |
|---|---|
| Over-redaction removes signal needed for correlation | Keep domain, repo-relative path, command name; measure assignment rate in M5 |
| Schema churn breaks deployed agents | Accept N and N-1; additive changes preferred |
