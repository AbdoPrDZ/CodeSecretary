//! `secretary-agent probe` — Phase 0 prototype (AG-0.1).
//!
//! Prints presence (active/idle) and focus changes live to the terminal so
//! the OS integrations can be validated by hand on each OS. Output goes to
//! stdout only: nothing is logged, stored, or sent.

use std::io::Write;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crate::cli::ProbeArgs;
use crate::platform::{self, FocusedWindow, Platform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Active,
    Idle,
}

/// The user is idle once there has been no input for `threshold`.
pub fn classify(idle: Duration, threshold: Duration) -> Presence {
    if idle >= threshold {
        Presence::Idle
    } else {
        Presence::Active
    }
}

/// Remembers the last reported value and says whether a new one is a change.
#[derive(Debug)]
struct Changes<T> {
    last: Option<T>,
}

impl<T> Default for Changes<T> {
    fn default() -> Self {
        Self { last: None }
    }
}

impl<T: PartialEq> Changes<T> {
    fn update(&mut self, value: T) -> bool {
        if self.last.as_ref() == Some(&value) {
            false
        } else {
            self.last = Some(value);
            true
        }
    }
}

pub fn run(args: ProbeArgs) -> ExitCode {
    let mut platform = match Platform::connect() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("secretary-agent probe: {err}");
            return ExitCode::from(1);
        }
    };

    let threshold = Duration::from_secs(args.idle_after);
    let interval = Duration::from_millis(args.interval_ms.max(100));

    println!("CodeSecretary probe — presence and focus, live. Ctrl+C to stop.");
    println!("Output stays in this terminal: nothing is logged, stored, or sent.");
    println!(
        "os: {} · idle after {}s without input · sampling every {} ms",
        std::env::consts::OS,
        threshold.as_secs(),
        interval.as_millis()
    );
    for note in platform::session_notes() {
        println!("note: {note}");
    }
    if !platform.titles_available() {
        println!("note: window titles unavailable — showing app names only");
    }
    println!();

    let start = Instant::now();
    let mut presence = Changes::default();
    let mut focus: Changes<Option<FocusedWindow>> = Changes::default();
    let mut errors: Changes<String> = Changes::default();
    let mut samples = 0u64;

    loop {
        let now = start.elapsed();

        match platform.idle_time() {
            Ok(idle) => {
                let state = classify(idle, threshold);
                if presence.update(state) {
                    match state {
                        Presence::Active => {
                            println!("{}  presence  ACTIVE", clock(now));
                        }
                        Presence::Idle => println!(
                            "{}  presence  IDLE    (no input for {}s)",
                            clock(now),
                            idle.as_secs()
                        ),
                    }
                }
            }
            Err(err) => {
                if errors.update(err.to_string()) {
                    println!("{}  error     idle: {err}", clock(now));
                }
            }
        }

        match platform.focused_window() {
            Ok(window) => {
                if focus.update(window.clone()) {
                    println!(
                        "{}  focus     {}",
                        clock(now),
                        describe(window.as_ref(), args.no_titles)
                    );
                }
            }
            Err(err) => {
                if errors.update(err.to_string()) {
                    println!("{}  error     focus: {err}", clock(now));
                }
            }
        }
        let _ = std::io::stdout().flush();

        samples += 1;
        if args.samples.is_some_and(|max| samples >= max) {
            return ExitCode::SUCCESS;
        }
        std::thread::sleep(interval);
    }
}

fn describe(window: Option<&FocusedWindow>, hide_title: bool) -> String {
    let Some(window) = window else {
        return "(no foreground window)".to_owned();
    };
    let app = window.app.as_deref().unwrap_or("(unknown app)");
    let pid = window
        .pid
        .map(|p| format!(" [pid {p}]"))
        .unwrap_or_default();
    let title = match (&window.title, hide_title) {
        (_, true) => String::new(),
        (Some(t), false) => format!(" — {}", truncate(t, 90)),
        (None, false) => " — (no title)".to_owned(),
    };
    format!("{app}{pid}{title}")
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_owned()
    } else {
        let mut out: String = text.chars().take(max_chars - 1).collect();
        out.push('…');
        out
    }
}

/// Elapsed time since the probe started, as `HH:MM:SS`.
fn clock(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_uses_threshold_inclusively() {
        let t = Duration::from_secs(300);
        assert_eq!(classify(Duration::from_secs(0), t), Presence::Active);
        assert_eq!(classify(Duration::from_secs(299), t), Presence::Active);
        assert_eq!(classify(Duration::from_secs(300), t), Presence::Idle);
    }

    #[test]
    fn changes_reports_only_transitions() {
        let mut c = Changes::default();
        assert!(c.update(Presence::Active));
        assert!(!c.update(Presence::Active));
        assert!(c.update(Presence::Idle));
        assert!(c.update(Presence::Active));
    }

    #[test]
    fn describe_formats_and_hides_titles() {
        let w = FocusedWindow {
            app: Some("Code.exe".into()),
            title: Some("main.rs — Secretary".into()),
            pid: Some(42),
        };
        assert_eq!(
            describe(Some(&w), false),
            "Code.exe [pid 42] — main.rs — Secretary"
        );
        assert_eq!(describe(Some(&w), true), "Code.exe [pid 42]");
        assert_eq!(describe(None, false), "(no foreground window)");
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("héllo", 10), "héllo");
        assert_eq!(truncate("ééééé", 3), "éé…");
    }

    #[test]
    fn clock_formats_hours_minutes_seconds() {
        assert_eq!(clock(Duration::from_secs(3723)), "01:02:03");
    }
}
