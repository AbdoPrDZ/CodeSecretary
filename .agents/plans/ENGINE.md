# Service Plan — Engine (`engine/`)

> **Blueprint:** [IDEA-V4.md](../blueprints/IDEA-V4.md) §10, §11, §12, Appendix A · **General plan:** [GENERAL.md](GENERAL.md)

## 1. Purpose

A background worker that turns raw events into **project assignments** and **work sessions**, deterministically and explainably. It is the **only** component that computes time.

## 2. Structure

```
engine/
├─ Cargo.toml                 # secretary-engine
├─ fixtures/                  # golden test days: events.json + expected_sessions.json
└─ src/
   ├─ main.rs                 # worker loop, concurrency, shutdown
   ├─ queue.rs                # claim jobs (FOR UPDATE SKIP LOCKED), retry, dead-letter
   ├─ load.rs                 # load a (user, local day) context: events, rules, overrides, adjustments, config
   ├─ correlate/
   │  ├─ mod.rs               # resolution order (blueprint §10.1)
   │  ├─ rules.rs             # personal + shared mapping rules
   │  ├─ structural.rs        # repo root, workspace, cwd
   │  └─ temporal.rs          # context-window inheritance
   ├─ calculate/
   │  ├─ presence.rs          # per-device presence → merged timeline
   │  ├─ credit.rs            # event credit (Appendix A)
   │  ├─ overlap.rs           # priority resolution
   │  ├─ sessions.rs          # merge, min length, waiting time
   │  ├─ gaps.rs              # coverage gaps from heartbeats
   │  └─ adjust.rs            # apply time_adjustments
   ├─ write.rs                # transactional replace of derived rows for (user, day)
   ├─ proposals.rs            # rule proposals from corrections (Phase 2)
   └─ ai/                     # Phase 2: provider client, cache, budget, logging
```

## 3. Processing Model

```
claim job (user_id, local_day)
   └─► load context (day ± 1 h margin for sessions crossing midnight)
        └─► correlate events → assignments
             └─► calculate → sessions (active / waiting) + evidence + gaps
                  └─► apply adjustments
                       └─► write (single transaction: delete derived rows for day, insert new)
                            └─► mark job done; if day is submitted and totals changed → flag resubmission (Phase 3)
```

- **Pure core:** `correlate` and `calculate` are pure functions of the loaded context — no DB, no clock, no randomness. This makes golden tests and determinism trivial.
- **Idempotent:** running the same job twice yields identical rows.
- **Versioned:** `ALGORITHM_VERSION` constant + `config_hash` (hash of effective settings) stored on every session.
- **Concurrency:** N workers; one job per (user, day) at a time (claim lock).
- **Retries:** exponential backoff, max 5; then `failed` with error for inspection.
- **Triggers** (all enqueue the same job type): new events (Warehouse), rule/override/adjustment/config change (Overview), algorithm upgrade (Engine startup enqueues recent days, e.g. last 30).

## 4. Algorithm Contract (blueprint §11.2)

1. Presence timeline per device → merged (present if any device present).
2. Credit events per Appendix A; interval events use actual duration.
3. Clip to presence; background tasks during idle → waiting-time candidates.
4. Overlap: most recently active device → focused window → VS Code workspace → terminal → browser → file system.
5. Merge same-project gaps ≤ 10 min (if present).
6. Drop sessions < 1 min.
7. Coverage gaps (missing heartbeats > 3 min) → affected sessions flagged `low_confidence`.
8. Apply adjustments (add, trim, split, reassign, delete), anchored by time range + project.

Each session stores its evidence event IDs; each assignment stores method, confidence, and rule ID.

## 5. Tasks

### Phase 1

| ID | Task | Milestone | Acceptance |
|---|---|---|---|
| EN-1 | Crate skeleton, worker loop, tracing, health | M0 | Boots in Docker Compose |
| EN-2 | Job queue: claim, heartbeat, retry, dead-letter | M3 | Two workers never process the same (user, day) concurrently |
| EN-3 | Context loader (events, rules, overrides, adjustments, time zone, config) | M3 | Handles sessions crossing midnight |
| EN-4 | Correlation: user override, personal rules, structural, temporal, unassigned | M3 | Unit tests per step; resolution order respected |
| EN-5 | Presence timeline (multi-device merge) | M3 | Golden tests incl. two devices |
| EN-6 | Event credit + clip to presence | M3 | Golden tests |
| EN-7 | Overlap resolution | M3 | Golden tests for every priority pair |
| EN-8 | Session building (merge gap, min length, waiting time) | M3 | Golden tests incl. long build while idle |
| EN-9 | Coverage gaps from heartbeats | M3 | Offline device → `low_confidence` sessions |
| EN-10 | Adjustments application | M3 | Adjustments survive recalculation |
| EN-11 | Transactional writer + versioning (`algorithm_version`, `config_hash`) | M3 | Re-run → byte-identical output |
| EN-12 | Golden fixture set (≥ 15 realistic days incl. Phase 0 baseline days) | M3 | CI runs all goldens |
| EN-13 | Git backfill handling (`estimated` sessions, 30 min before commit) | M3 | Estimated sessions marked and excluded from accuracy metrics |
| EN-14 | Algorithm-upgrade re-enqueue on startup | M3 | New version recalculates last 30 days |
| EN-15 | Accuracy report tool: compare sessions vs. manual baseline CSV | M5 | Outputs per-project deviation; Phase 1 target ±15% |
| EN-16 | Performance: one user-day < 500 ms p95 | M5 | Benchmarked on a heavy day (10k events) |

### Phase 2

| ID | Task |
|---|---|
| EN-20 | Credit rules for terminal, browser, fs events; VS Code terminal dedupe |
| EN-21 | Rule proposals from correction patterns |
| EN-22 | AI provider abstraction (cloud API, local model server), config per user |
| EN-23 | AI classification for ambiguous events: batching, caching by input hash, confidence ≤ 0.8 |
| EN-24 | AI budget per user/workspace; over budget → leave unassigned |
| EN-25 | AI request logging (`ai_request_log`) with exact fields sent |
| EN-26 | Grounded daily/weekly summaries with evidence references |
| EN-27 | Browser research association (docs/issues/Q&A domains → active project) |
| EN-28 | Android events: phone presence and app focus credit only when mapped to a project (package-name mappings); desktop wins overlaps (D16) |

### Phase 3

| ID | Task |
|---|---|
| EN-30 | Shared (workspace) mapping rules in resolution order (after personal rules) |
| EN-31 | Submitted-day protection: detect changed totals → resubmission flag + notification |
| EN-32 | Workspace AI policy (stricter of user/workspace wins) |

### Phase 4

| ID | Task |
|---|---|
| EN-40 | Billable/waiting-time policy and rounding rules applied in report aggregates |

## 6. Testing

- **Golden tests** are the core: `fixtures/<case>/events.json` + `config.json` → `expected_sessions.json`. Any change in output requires updating goldens in the same PR, with `ALGORITHM_VERSION` bump if behaviour intentionally changed.
- Property tests: total active time ≤ total presence time; no two sessions overlap for one user; every session has ≥ 1 evidence event.
- Determinism test: run each fixture 10× → identical output.
- Integration: Warehouse ingest → job → Engine → derived rows (testcontainers).

## 7. Invariants (asserted in code and tests)

- No overlapping active sessions for a user.
- Active time never exceeds presence time.
- Every session has evidence.
- AI never sets start/end times.
- Output depends only on the loaded context.

## 8. Risks

| Risk | Mitigation |
|---|---|
| Under-counting reading/thinking | Tune merge gap / focus credit using baseline; accuracy tool |
| Recalculation storms (rule change → many days) | Batch enqueue with priority; recent days first |
| Midnight / DST edge cases | Load ±1 h margin; tests in DST-changing time zones |
