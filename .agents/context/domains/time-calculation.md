# Domain — Time Calculation

Status: engine v1 implemented (`ALGORITHM_VERSION` 1: rules, temporal, presence, credit, overlap, sessions); coverage gaps, adjustments, waiting time and Git backfill planned. Source: `engine/src/{correlate,calculate,model}.rs`; blueprint §10, §11, Appendix A; [ENGINE plan](../../plans/ENGINE.md).

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

As implemented (v1):
- **Structural is skipped:** under D17 only the user's rules bind evidence to projects; detected repositories are suggestions.
- **Temporal (0.7):** an unmatched evidence event whose nearest directly decided neighbours (override or rule, never another temporal one) on both sides, within the 5 min context window, have the same project.
- **Evidence events:** focused windows, editor events and Git commit/checkout/merge. Presence and agent events are inputs, not evidence. A user override without a project ("not project work") credits nothing.
- Only assigned events get an `event_assignments` row; an event without one is unassigned.

## Calculation Steps

Presence timeline → event credit → clip to presence (idle background → waiting) → overlap resolution (most recently active device, then focused window → VS Code → terminal → browser → file system) → merge gaps ≤ merge gap → drop short sessions → flag coverage gaps → apply adjustments. Default numbers: blueprint Appendix A.

As implemented (v1), where it differs or adds detail:
- **Presence:** `presence.active` of all devices merged. Gaps up to 4 min count as present: the agent marks idle after 60 s, the blueprint's idle threshold is 5 min.
- **Credit:** intervals use their duration. An edit burst counts at least 2 min up to its end. A save, commit, checkout or merge counts the 2 min before it.
- **Overlap:** decided (assigned or user-decided) beats unassigned, so a path rule on the editor wins over the editor's own unmatched window. Then focused window → editor → Git, then the latest start. The device with the most recent input is not used yet.
- **Merge** needs the gap to be present. Merging happens before short sessions are dropped, so a sub-minute switch still splits a session.
- **Day ownership:** sessions are cut at the local day's bounds (user's `time_zone`). Each day owns exactly its own time, and the ±1 h margin only gives context.
- Unassigned time becomes sessions with no project, so it is visible and correctable.
- **Adjustments (v2):** applied after the day cut, in id order, clipped to the day.
  - `remove` drops matching parts; `reassign` moves them; matching means the from-project, or any project when none is set.
  - `add` replaces the range with a manual session.
  - Touching same-project pieces are then joined.
  - Manual sessions have no evidence and do not count against presence in the invariants.

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
