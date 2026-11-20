# Progress — Engine (`engine/`)

> **Plan:** [ENGINE.md](../plans/ENGINE.md) · **Overall:** [GENERAL.progress.md](GENERAL.progress.md)
>
> Tick only when acceptance criteria are met. Format: `- [x] ID — task · YYYY-MM-DD · note`. In progress: `· 🚧`. Blocked: `· ⛔ reason`.

**Current `ALGORITHM_VERSION`:** — (not released)

## Phase 1

### M0
- [ ] EN-1 — Crate skeleton, worker loop, tracing, health
  - Acceptance: boots in Docker Compose

### M3
- [ ] EN-2 — Job queue: claim, heartbeat, retry, dead-letter
  - Acceptance: two workers never process the same (user, day) concurrently
- [ ] EN-3 — Context loader
  - Acceptance: handles sessions crossing midnight
- [ ] EN-4 — Correlation: override, personal rules, structural, temporal, unassigned
  - Acceptance: unit tests per step; resolution order respected
- [ ] EN-5 — Presence timeline (multi-device merge)
  - Acceptance: golden tests incl. two devices
- [ ] EN-6 — Event credit + clip to presence
  - Acceptance: golden tests
- [ ] EN-7 — Overlap resolution
  - Acceptance: golden tests for every priority pair
- [ ] EN-8 — Session building (merge gap, min length, waiting time)
  - Acceptance: golden tests incl. long build while idle
- [ ] EN-9 — Coverage gaps from heartbeats
  - Acceptance: offline device → `low_confidence` sessions
- [ ] EN-10 — Adjustments application
  - Acceptance: adjustments survive recalculation
- [ ] EN-11 — Transactional writer + versioning
  - Acceptance: re-run → byte-identical output
- [ ] EN-12 — Golden fixture set (≥ 15 realistic days)
  - Acceptance: CI runs all goldens
- [ ] EN-13 — Git backfill handling (`estimated` sessions)
  - Acceptance: estimated sessions marked and excluded from accuracy metrics
- [ ] EN-14 — Algorithm-upgrade re-enqueue on startup
  - Acceptance: new version recalculates last 30 days

### M5
- [ ] EN-15 — Accuracy report tool vs. manual baseline CSV
  - Acceptance: per-project deviation; target ±15%
- [ ] EN-16 — Performance: one user-day < 500 ms p95
  - Acceptance: benchmarked on a 10k-event day

### Invariants (tests in place)
- [ ] No overlapping active sessions for a user
- [ ] Active time never exceeds presence time
- [ ] Every session has evidence
- [ ] AI never sets start/end times
- [ ] Output depends only on the loaded context (determinism test)

## Phase 2
- [ ] EN-20 — Credit rules for terminal, browser, fs; VS Code terminal dedupe
- [ ] EN-21 — Rule proposals from correction patterns
- [ ] EN-22 — AI provider abstraction (cloud, local)
- [ ] EN-23 — AI classification: batching, caching, confidence ≤ 0.8
- [ ] EN-24 — AI budget per user/workspace
- [ ] EN-25 — AI request logging
- [ ] EN-26 — Grounded daily/weekly summaries
- [ ] EN-27 — Browser research association
- [ ] EN-28 — Android events: credit only when mapped; desktop wins overlaps (D16)

## Phase 3
- [ ] EN-30 — Shared mapping rules in resolution order
- [ ] EN-31 — Submitted-day protection + resubmission flag
- [ ] EN-32 — Workspace AI policy (stricter wins)

## Phase 4
- [ ] EN-40 — Billable/waiting-time policy and rounding in aggregates

## Accuracy Log

| Date | Algorithm version | Baseline period | Deviation | Auto-assigned |
|---|---|---|---|---|

## Notes
