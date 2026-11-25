//! Correlation: which project each evidence event belongs to (blueprint §10.1).
//!
//! Resolution order, first match wins:
//!
//! 1. User override (confidence 1.0).
//! 2. Personal mapping rule (0.95).
//! 3. Shared (workspace) mapping rule (0.95).
//! 4. Structural signals: not used. Projects are bound to evidence only by
//!    the user's rules (D17); detected repositories are only suggestions.
//! 5. Temporal context (0.7): an unmatched event whose nearest decided
//!    neighbours on both sides, within the context window, belong to the
//!    same project (e.g. a terminal or browser window between two editor
//!    events of one repository).
//! 6. AI: Phase 2.
//! 7. Unassigned.
//!
//! Rules use the same semantics as the dashboard preview
//! ([`secretary_shared::rules`]).

use secretary_shared::Payload;
use secretary_shared::rules::{Evidence, Rule, resolve};

use crate::model::{Context, Method, Settings, Span};

/// How an event counts towards time. Lower wins overlaps (§11.2 step 4):
/// focused window, then the editor, then Git.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Focus = 0,
    Editor = 1,
    Git = 2,
}

/// The category of an evidence event; `None` for presence, agent and
/// backfill events (those are not evidence of what was worked on).
pub fn category(payload: &Payload) -> Option<Category> {
    match payload {
        Payload::WindowFocus(_) => Some(Category::Focus),
        Payload::EditorWorkspaceOpen(_)
        | Payload::EditorFileFocus(_)
        | Payload::EditorEditBurst(_)
        | Payload::EditorSave(_)
        | Payload::EditorDebugSession(_) => Some(Category::Editor),
        Payload::GitCommit(_) | Payload::GitCheckout(_) | Payload::GitMerge(_) => {
            Some(Category::Git)
        }
        _ => None,
    }
}

/// The correlation result for one event.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Resolved {
    /// `Some` once a step decided; the project may still be `None` (a user
    /// override saying "not project work").
    pub method: Option<Method>,
    pub project: Option<i64>,
    pub confidence: f64,
    pub rule_id: Option<i64>,
}

impl Resolved {
    pub fn decided(&self) -> bool {
        self.method.is_some()
    }
}

/// One [`Resolved`] per `ctx.events` entry (same order).
pub fn correlate(ctx: &Context) -> Vec<Resolved> {
    let personal: Vec<Rule> = ctx
        .rules
        .iter()
        .filter(|r| !r.shared)
        .map(|r| r.rule.clone())
        .collect();
    let shared: Vec<Rule> = ctx
        .rules
        .iter()
        .filter(|r| r.shared)
        .map(|r| r.rule.clone())
        .collect();

    let mut out: Vec<Resolved> = ctx
        .events
        .iter()
        .map(|e| {
            if category(&e.event.payload).is_none() {
                return Resolved::default();
            }
            if let Some(project) = ctx.overrides.get(&e.id()) {
                return Resolved {
                    method: Some(Method::User),
                    project: *project,
                    confidence: 1.0,
                    rule_id: None,
                };
            }
            let evidence = Evidence::from_event(&e.event);
            for (rules, method) in [(&personal, Method::Rule), (&shared, Method::SharedRule)] {
                if let Some(rule) = resolve(rules, &evidence) {
                    return Resolved {
                        method: Some(method),
                        project: Some(rule.project_id),
                        confidence: 0.95,
                        rule_id: Some(rule.id),
                    };
                }
            }
            Resolved::default()
        })
        .collect();

    temporal(ctx, &mut out);
    out
}

/// Step 5. Only directly decided events (steps 1–3) with a project count as
/// neighbours, so inherited assignments never chain.
fn temporal(ctx: &Context, out: &mut [Resolved]) {
    let window = Settings::secs(ctx.settings.context_window_secs);
    let spans: Vec<Span> = ctx.events.iter().map(|e| e.span()).collect();
    let anchors: Vec<usize> = (0..out.len())
        .filter(|&i| out[i].decided() && out[i].project.is_some())
        .collect();
    let targets: Vec<usize> = (0..out.len())
        .filter(|&i| !out[i].decided() && category(&ctx.events[i].event.payload).is_some())
        .collect();
    if anchors.is_empty() || targets.is_empty() {
        return;
    }

    // Before: among anchors starting at or before the target's start, the
    // one that ends last (ties: first in event order).
    let mut by_start = anchors.clone();
    by_start.sort_by_key(|&i| (spans[i].start, i));
    let mut targets_by_start = targets.clone();
    targets_by_start.sort_by_key(|&i| (spans[i].start, i));
    let mut before = vec![None; out.len()];
    let (mut k, mut best): (usize, Option<usize>) = (0, None);
    for &t in &targets_by_start {
        while k < by_start.len() && spans[by_start[k]].start <= spans[t].start {
            let a = by_start[k];
            if best.is_none_or(|b| spans[a].end > spans[b].end) {
                best = Some(a);
            }
            k += 1;
        }
        before[t] = best.filter(|&b| spans[b].end >= spans[t].start - window);
    }

    // After: among anchors ending at or after the target's end, the one that
    // starts first (ties: first in event order).
    let mut by_end = anchors;
    by_end.sort_by_key(|&i| (std::cmp::Reverse(spans[i].end), i));
    let mut targets_by_end = targets;
    targets_by_end.sort_by_key(|&i| (std::cmp::Reverse(spans[i].end), i));
    let (mut k, mut best): (usize, Option<usize>) = (0, None);
    for &t in &targets_by_end {
        while k < by_end.len() && spans[by_end[k]].end >= spans[t].end {
            let a = by_end[k];
            if best.is_none_or(|b| spans[a].start < spans[b].start) {
                best = Some(a);
            }
            k += 1;
        }
        let after = best.filter(|&b| spans[b].start <= spans[t].end + window);
        if let (Some(b), Some(a)) = (before[t], after)
            && out[b].project == out[a].project
        {
            out[t] = Resolved {
                method: Some(Method::Temporal),
                project: out[b].project,
                confidence: 0.7,
                rule_id: None,
            };
        }
    }
}
