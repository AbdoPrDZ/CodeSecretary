//! Time calculation (blueprint §11.2) for one (user, local day).
//!
//! 1. Presence: `presence.active` of every device, merged; gaps up to
//!    `presence_gap_fill_secs` count as present.
//! 2. Credit (Appendix A): intervals (focused window, file focus, debug
//!    session) use their duration; an edit burst counts at least
//!    `point_credit_secs` up to its end; a save, commit, checkout or merge
//!    counts `point_credit_secs` before it.
//! 3. Overlaps: at every instant one event wins: decided before unassigned
//!    (a rule match outranks an unmatched window, so a path rule on the
//!    editor wins over the editor's own unmatched window), then focused
//!    window → editor → Git, then the latest started, then the lowest id.
//! 4. Clip to presence.
//! 5. Merge same-project pieces separated by ≤ `merge_gap_secs` of presence.
//! 6. Cut at the local day's bounds (each day owns exactly its own time),
//!    then drop sessions shorter than `min_session_secs`.
//! 7. Apply the user's adjustments in the order they were made ([`adjust`]),
//!    then join touching pieces of the same project.
//!
//! Not yet: multi-device "most recent input" priority, waiting time
//! (needs terminal events, Phase 2), coverage gaps (EN-9), Git backfill
//! (EN-13).

use std::collections::BTreeSet;

use secretary_shared::Payload;
use uuid::Uuid;

use crate::correlate::{Category, Resolved, category, correlate};
use crate::model::{
    Adjustment, AdjustmentKind, Assignment, Context, Kind, Output, Session, Settings, Span, Time,
};

/// Correlate and calculate: the whole engine for one day, without I/O.
pub fn run(ctx: &Context) -> Output {
    let mut events: Vec<usize> = (0..ctx.events.len()).collect();
    // Deterministic regardless of load order.
    events.sort_by_key(|&i| (ctx.events[i].event.timestamp_start, ctx.events[i].id()));
    let sorted = Context {
        events: events.iter().map(|&i| ctx.events[i].clone()).collect(),
        ..ctx.clone()
    };
    let resolved = correlate(&sorted);
    let day = sorted.day();

    let mut assignments: Vec<Assignment> = sorted
        .events
        .iter()
        .zip(&resolved)
        .filter(|(e, _)| e.event.timestamp_start >= day.start && e.event.timestamp_start < day.end)
        .filter_map(|(e, r)| {
            Some(Assignment {
                event_id: e.id(),
                project_id: r.project?,
                method: r.method?,
                confidence: r.confidence,
                rule_id: r.rule_id,
            })
        })
        .collect();
    assignments.sort_by_key(|a| a.event_id);

    let presence = presence(&sorted);
    let pieces = clip(&timeline(&sorted, &resolved), &presence);
    let sessions = adjust(
        sessions(&pieces, &presence, &sorted.settings, day),
        &sorted.adjustments,
        day,
    );
    Output {
        assignments,
        sessions,
    }
}

/// Merged presence of all devices within the context window, gaps filled.
pub fn presence(ctx: &Context) -> Vec<Span> {
    let window = ctx.window();
    let mut spans: Vec<Span> = ctx
        .events
        .iter()
        .filter(|e| matches!(e.event.payload, Payload::PresenceActive(_)))
        .filter_map(|e| e.span().intersect(&window))
        .collect();
    spans.sort();
    let fill = Settings::secs(ctx.settings.presence_gap_fill_secs);
    let mut out: Vec<Span> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(last) if s.start <= last.end + fill => last.end = last.end.max(s.end),
            _ => out.push(s),
        }
    }
    out
}

/// The time an event is credited with (step 2), or `None`.
pub fn credit(payload: &Payload, span: Span, settings: &Settings) -> Option<Span> {
    let pc = Settings::secs(settings.point_credit_secs);
    let s = match payload {
        Payload::WindowFocus(_) | Payload::EditorFileFocus(_) | Payload::EditorDebugSession(_) => {
            span
        }
        Payload::EditorEditBurst(_) => Span::new(span.start.min(span.end - pc), span.end),
        Payload::EditorSave(_)
        | Payload::GitCommit(_)
        | Payload::GitCheckout(_)
        | Payload::GitMerge(_) => Span::new(span.start - pc, span.start),
        _ => return None,
    };
    (!s.is_empty()).then_some(s)
}

/// A stretch of time won by one project (or unassigned).
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub span: Span,
    pub project: Option<i64>,
    pub evidence: BTreeSet<Uuid>,
}

struct Candidate {
    span: Span,
    decided: bool,
    category: Category,
    project: Option<i64>,
    id: Uuid,
}

impl Candidate {
    /// Smaller wins (step 3).
    fn rank(&self) -> (bool, Category, std::cmp::Reverse<Time>, Uuid) {
        (
            !self.decided,
            self.category,
            std::cmp::Reverse(self.span.start),
            self.id,
        )
    }
}

/// Steps 2–3: credited spans → non-overlapping pieces, in time order.
pub fn timeline(ctx: &Context, resolved: &[Resolved]) -> Vec<Piece> {
    let window = ctx.window();
    let candidates: Vec<Candidate> = ctx
        .events
        .iter()
        .zip(resolved)
        .filter_map(|(e, r)| {
            let category = category(&e.event.payload)?;
            let span = credit(&e.event.payload, e.span(), &ctx.settings)?.intersect(&window)?;
            // "Not project work" (user override without a project) credits nothing.
            if r.method.is_some() && r.project.is_none() {
                return None;
            }
            Some(Candidate {
                span,
                decided: r.decided(),
                category,
                project: r.project,
                id: e.id(),
            })
        })
        .collect();

    // Sweep over every boundary; the active set is small (a few overlapping events).
    let mut bounds: Vec<(Time, bool, usize)> = Vec::with_capacity(candidates.len() * 2);
    for (i, c) in candidates.iter().enumerate() {
        bounds.push((c.span.start, true, i));
        bounds.push((c.span.end, false, i));
    }
    bounds.sort_by_key(|&(t, is_start, i)| (t, is_start, i));

    let mut active: Vec<usize> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut k = 0;
    while k < bounds.len() {
        let t = bounds[k].0;
        while k < bounds.len() && bounds[k].0 == t {
            let (_, is_start, i) = bounds[k];
            if is_start {
                active.push(i);
            } else {
                active.retain(|&a| a != i);
            }
            k += 1;
        }
        let Some(&next) = bounds.get(k).map(|(t, _, _)| t) else {
            break;
        };
        let Some(&winner) = active.iter().min_by_key(|&&i| candidates[i].rank()) else {
            continue;
        };
        let w = &candidates[winner];
        let evidence: BTreeSet<Uuid> = active
            .iter()
            .map(|&i| &candidates[i])
            .filter(|c| c.project == w.project && c.decided == w.decided)
            .map(|c| c.id)
            .collect();
        let span = Span::new(t, next);
        match pieces.last_mut() {
            Some(last) if last.span.end == t && last.project == w.project => {
                last.span.end = next;
                last.evidence.extend(evidence);
            }
            _ => pieces.push(Piece {
                span,
                project: w.project,
                evidence,
            }),
        }
    }
    pieces
}

/// Step 4: keep only the parts of pieces inside presence (both sorted).
pub fn clip(pieces: &[Piece], presence: &[Span]) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut j = 0;
    for p in pieces {
        while j < presence.len() && presence[j].end <= p.span.start {
            j += 1;
        }
        let mut k = j;
        while k < presence.len() && presence[k].start < p.span.end {
            if let Some(span) = p.span.intersect(&presence[k]) {
                out.push(Piece {
                    span,
                    project: p.project,
                    evidence: p.evidence.clone(),
                });
            }
            k += 1;
        }
    }
    out
}

fn covered(span: Span, presence: &[Span]) -> bool {
    span.is_empty()
        || presence
            .iter()
            .any(|p| p.start <= span.start && p.end >= span.end)
}

/// Steps 5–6.
pub fn sessions(
    pieces: &[Piece],
    presence: &[Span],
    settings: &Settings,
    day: Span,
) -> Vec<Session> {
    let gap = Settings::secs(settings.merge_gap_secs);
    let mut merged: Vec<Piece> = Vec::new();
    for p in pieces {
        match merged.last_mut() {
            Some(last)
                if last.project == p.project
                    && p.span.start - last.span.end <= gap
                    && covered(Span::new(last.span.end, p.span.start), presence) =>
            {
                last.span.end = p.span.end;
                last.evidence.extend(p.evidence.iter().copied());
            }
            _ => merged.push(p.clone()),
        }
    }
    let min = Settings::secs(settings.min_session_secs);
    merged
        .into_iter()
        .filter_map(|p| {
            let span = p.span.intersect(&day)?;
            (span.len() >= min).then(|| Session {
                project_id: p.project,
                kind: Kind::Active,
                start: span.start,
                end: span.end,
                low_confidence: false,
                estimated: false,
                manual: false,
                evidence: p.evidence.into_iter().collect(),
            })
        })
        .collect()
}

/// Step 7: the user's corrections (`time_adjustments`), clipped to the day.
///
/// - `remove` drops the range from matching sessions (trim, delete).
/// - `reassign` moves the matching parts of the range to `project_id`
///   (reassign, split).
/// - `add` replaces whatever the range held with manual time.
///
/// "Matching" = every session, or only `from_project_id`'s when set.
/// Corrections are anchored to time and project, never to session ids, so
/// they survive recalculation. Touching pieces of the same project are
/// joined afterwards.
pub fn adjust(mut sessions: Vec<Session>, adjustments: &[Adjustment], day: Span) -> Vec<Session> {
    let mut ordered: Vec<&Adjustment> = adjustments.iter().collect();
    ordered.sort_by_key(|a| a.id);
    for a in ordered {
        let Some(range) = Span::new(a.start, a.end).intersect(&day) else {
            continue;
        };
        let matches = |s: &Session| match a.kind {
            AdjustmentKind::Add => true,
            _ => a.from_project_id.is_none_or(|f| s.project_id == Some(f)),
        };
        let mut next = Vec::with_capacity(sessions.len() + 2);
        for s in sessions {
            let span = Span::new(s.start, s.end);
            let Some(inside) = span.intersect(&range).filter(|_| matches(&s)) else {
                next.push(s);
                continue;
            };
            let piece = |from: Time, to: Time| Session {
                start: from,
                end: to,
                ..s.clone()
            };
            if s.start < inside.start {
                next.push(piece(s.start, inside.start));
            }
            if a.kind == AdjustmentKind::Reassign {
                next.push(Session {
                    project_id: a.project_id,
                    ..piece(inside.start, inside.end)
                });
            }
            if inside.end < s.end {
                next.push(piece(inside.end, s.end));
            }
        }
        if a.kind == AdjustmentKind::Add {
            next.push(Session {
                project_id: a.project_id,
                kind: Kind::Active,
                start: range.start,
                end: range.end,
                low_confidence: false,
                estimated: false,
                manual: true,
                evidence: Vec::new(),
            });
        }
        next.sort_by_key(|s| s.start);
        sessions = next;
    }

    let mut joined: Vec<Session> = Vec::with_capacity(sessions.len());
    for s in sessions {
        match joined.last_mut() {
            Some(last)
                if last.end == s.start
                    && last.project_id == s.project_id
                    && last.kind == s.kind
                    && last.manual == s.manual =>
            {
                last.end = s.end;
                last.low_confidence |= s.low_confidence;
                last.estimated |= s.estimated;
                let mut evidence: BTreeSet<Uuid> = last.evidence.drain(..).collect();
                evidence.extend(s.evidence);
                last.evidence = evidence.into_iter().collect();
            }
            _ => joined.push(s),
        }
    }
    joined
}

/// The invariants of ENGINE §7 that hold for this output (tests assert them).
/// Manual sessions (user-added time) have no evidence and do not count
/// against presence.
pub fn check_invariants(ctx: &Context, out: &Output) -> Result<(), String> {
    let mut prev: Option<&Session> = None;
    for s in &out.sessions {
        if s.end <= s.start {
            return Err(format!("empty session at {}", s.start));
        }
        if s.evidence.is_empty() && !s.manual {
            return Err(format!("session at {} has no evidence", s.start));
        }
        if let Some(p) = prev
            && p.end > s.start
        {
            return Err(format!("sessions overlap at {}", s.start));
        }
        prev = Some(s);
    }
    let day = ctx.day();
    let present: chrono::TimeDelta = presence(ctx)
        .iter()
        .filter_map(|p| p.intersect(&day))
        .map(|p| p.len())
        .sum();
    let active: chrono::TimeDelta = out
        .sessions
        .iter()
        .filter(|s| !s.manual)
        .map(|s| s.end - s.start)
        .sum();
    if active > present {
        return Err(format!("active time {active} exceeds presence {present}"));
    }
    Ok(())
}
