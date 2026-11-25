# Progress — Engine (`engine/`)

> **Plan:** [ENGINE.md](../plans/ENGINE.md) · **Overall:** [GENERAL.progress.md](GENERAL.progress.md)
>
> Tick only when acceptance criteria are met. Format: `- [x] ID — task · YYYY-MM-DD · note`. In progress: `· 🚧`. Blocked: `· ⛔ reason`.

**Current `ALGORITHM_VERSION`:** 2 (2026-11-25, adjustments; 1 = engine v1)

## Phase 1

### M0
- [x] EN-1 — Crate skeleton, worker loop, tracing, health · 2026-11-25 · `secretary-engine` (lib + bin): env config, JSON tracing, N workers + reaper, SIGTERM → 20 s grace, `/healthz` + `/readyz` on :8070; image `engine/Dockerfile`; boots healthy in Docker Compose and stops gracefully (verified locally); CI `engine` job + Compose job added (not yet run: nothing pushed)
  - Acceptance: boots in Docker Compose

### M3
- [x] EN-2 — Job queue: claim, heartbeat, retry, dead-letter · 2026-11-25 · `queue.rs`: claim skips days with a running job (`SKIP LOCKED`) + partial unique index `processing_jobs_running_ux` as a hard guarantee; heartbeat 30 s, reaper reclaims after 5 min; backoff 10 s → 30 min, `failed` after 5 attempts (error kind only); a failure merges into a newer queued job of the same day; DB test: 8 workers, 3 users × 4 days re-queued while running → no overlap
  - Acceptance: two workers never process the same (user, day) concurrently
- [ ] EN-3 — Context loader · 🚧 in progress · 2026-11-25 · `load.rs`: events in the day ±1 h (user time zone, DST-aware bounds), personal + shared rules of active projects, overrides, all under RLS; DB test reads only the user's events; midnight handled in the pure core (golden `midnight-time-zone`); left: a DB test with events across midnight, adjustments (EN-10)
  - Acceptance: handles sessions crossing midnight
- [ ] EN-4 — Correlation: override, personal rules, structural, temporal, unassigned · 🚧 in progress · 2026-11-25 · `correlate.rs`: override → personal rule → shared rule → temporal (0.7) → unassigned; structural skipped (D17: rules only); covered by goldens; left: unit tests per step
  - Acceptance: unit tests per step; resolution order respected
- [x] EN-5 — Presence timeline (multi-device merge) · 2026-11-25 · union of all devices' `presence.active`, gaps ≤ 4 min count as present (agent idles at 60 s, blueprint threshold 5 min); golden `two-devices`
  - Acceptance: golden tests incl. two devices
- [x] EN-6 — Event credit + clip to presence · 2026-11-25 · intervals = duration; edit burst ≥ 2 min; save/commit/checkout/merge = 2 min before; clipped to presence; goldens `point-credit-and-unassigned`, `overrides-and-shared-rules`
  - Acceptance: golden tests
- [ ] EN-7 — Overlap resolution · 🚧 in progress · 2026-11-25 · decided beats unassigned, then focus → editor → Git, then latest start; goldens cover focus vs editor and decided vs unassigned; left: goldens for every pair, most-recent-input device priority
  - Acceptance: golden tests for every priority pair
- [ ] EN-8 — Session building (merge gap, min length, waiting time) · 🚧 in progress · 2026-11-25 · merge same project over ≤ 10 min of presence, drop < 1 min, cut at local-day bounds; golden `merge-gap-and-min-length`; left: waiting time (needs terminal events, Phase 2) and its golden
  - Acceptance: golden tests incl. long build while idle
- [ ] EN-9 — Coverage gaps from heartbeats
  - Acceptance: offline device → `low_confidence` sessions
- [x] EN-10 — Adjustments application · 2026-11-25 · `calculate::adjust`: the day's `time_adjustments` (not undone) in id order: `remove` (trim/delete), `reassign` (reassign/split), `add` (manual time, `work_sessions.manual`, no evidence), optional from-project filter, clipped to the day, touching same-project pieces joined; golden `adjustments`; DB test: a correction survives repeated recalculation, an undone one stops applying. `ALGORITHM_VERSION` 2
  - Acceptance: adjustments survive recalculation
- [x] EN-11 — Transactional writer + versioning · 2026-11-25 · `write.rs`: in the job's transaction (ownership re-checked with `FOR UPDATE`) delete + insert sessions, evidence, assignments; `algorithm_version` + `config_hash` (SHA-256 of version + settings); job marked done in the same commit; DB test: re-run → identical rows
  - Acceptance: re-run → byte-identical output
- [ ] EN-12 — Golden fixture set (≥ 15 realistic days) · 🚧 in progress · 2026-11-25 · 6 synthetic cases (`engine/fixtures/`, inputs from `gen_fixtures.py`, outputs reviewed by hand), determinism (10 shuffled runs) and invariants per case; left: ≥ 15 realistic days incl. Phase 0 baseline days
  - Acceptance: CI runs all goldens
- [ ] EN-13 — Git backfill handling (`estimated` sessions)
  - Acceptance: estimated sessions marked and excluded from accuracy metrics
- [x] EN-14 — Algorithm-upgrade re-enqueue on startup · 2026-11-25 · `app_enqueue_uncalculated_days(30, version)` on startup queues days with activity but no sessions of the current version (also first-time days); DB test: found before, none after, again for version + 1
  - Acceptance: new version recalculates last 30 days

### M5
- [ ] EN-15 — Accuracy report tool vs. manual baseline CSV
  - Acceptance: per-project deviation; target ±15%
- [ ] EN-16 — Performance: one user-day < 500 ms p95
  - Acceptance: benchmarked on a 10k-event day

### Invariants (tests in place)
- [x] No overlapping active sessions for a user · 2026-11-25 · `check_invariants` in every golden and before every write
- [x] Active time never exceeds presence time · 2026-11-25 · `check_invariants`
- [x] Every session has evidence · 2026-11-25 · `check_invariants`
- [ ] AI never sets start/end times
- [x] Output depends only on the loaded context (determinism test) · 2026-11-25 · golden runner: 10 runs with shuffled input order → identical output

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

- 2026-11-25: Rust is not installed on the dev machine; the engine is built and tested in the `rust:1-slim-trixie` container (Rust 1.100). sqlx 0.9.1 needs Rust 1.94, so the engine crate sets `rust-version = "1.94"` (the workspace stays at 1.89).
- 2026-11-25: v1 deviations from the blueprint, all recorded in `domains/time-calculation.md`: no structural step (D17), decided evidence outranks unassigned in overlaps, presence gaps ≤ 4 min count as present, sessions are cut at local-day bounds (each day owns its own time), unassigned time is kept as sessions without a project.
- 2026-11-25: only assigned events get `event_assignments` rows (the method column has no "unassigned" value); a user override to "not project work" credits nothing.
