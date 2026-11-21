# Domain — Time Calculation

Status: planned. Source: blueprint §10, §11, Appendix A; [ENGINE plan](../../plans/ENGINE.md).

## Purpose

Turn raw activity events into project **assignments** and **work sessions** that are deterministic, explainable, and correctable. This is the product's core; correctness here matters more than anywhere else.

## Key Concepts

- **Presence** — user is active on a device (from the Activity & Focus Watcher). The user is present if present on any device.
- **Assignment** — event → project, with `confidence` and `method` (`user`, `rule`, `shared_rule`, `structural`, `temporal`, `ai`).
- **Session** — continuous interval of one project for one user; type `active` or `waiting`.
- **Evidence** — events justifying a session.
- **Waiting time** — background task (e.g., build) running while the user is idle; reported separately.
- **Coverage gap** — collector/device offline; sessions there are flagged `low_confidence`, not silently counted or dropped.

## Correlation Order

Mapping rules (D17) are made by the user per project; semantics in `shared/src/rules.rs` (signals `path`, `remote`, `domain`, `app`, `title`; among matching rules the highest priority, then the longest pattern, then the oldest wins). Archived projects' rules are inactive.

User override → personal rule → shared (team) rule → structural (repo/workspace/cwd) → temporal context → AI (Phase 2, confidence ≤ 0.8) → unassigned (inbox). Below the review threshold → inbox even if assigned.

## Calculation Steps

Presence timeline → event credit → clip to presence (idle background → waiting) → overlap resolution (most recently active device, then focused window → VS Code → terminal → browser → file system) → merge gaps ≤ merge gap → drop short sessions → flag coverage gaps → apply adjustments. Default numbers: blueprint Appendix A.

## Rules and Invariants

- `correlate` and `calculate` are **pure functions** of the loaded day context: no DB, clock, randomness, or network inside them.
- Same inputs + config + `ALGORITHM_VERSION` → byte-identical output.
- No two active sessions of one user overlap.
- Active time ≤ presence time.
- Every session has ≥ 1 evidence event.
- AI may classify and summarize; it **never** sets start/end times or durations.
- User corrections are inputs, reapplied on every recalculation — never edits to derived rows.
- Git backfill sessions are marked `estimated` and excluded from accuracy metrics.
- Processing unit is (user, local day) with ±1 h margin for midnight-crossing sessions; DST must be handled.

## Changing This Domain

Any behavior change: update golden fixtures in `engine/fixtures/`, bump `ALGORITHM_VERSION` if output changes intentionally, and record accuracy in `ENGINE.progress.md`.
