use serde::Deserialize;

use super::*;
use crate::event::{EditorWorkspaceOpen, Empty, GitCheckout, GitCommit, Payload, WindowFocus};
use crate::{Event, RepositoryRef, Source};

/// The versioned corpus, also run through the agent pipeline (AG-6).
pub const CORPUS_JSON: &str = include_str!("../../fixtures/redaction-corpus-v1.json");

#[derive(Deserialize)]
struct Corpus {
    items: Vec<Item>,
    benign_titles: Vec<String>,
}

#[derive(Deserialize)]
struct Item {
    kind: String,
    template: String,
    secret: Vec<String>,
    #[serde(default)]
    titles_only: bool,
}

impl Item {
    fn secret(&self) -> String {
        self.secret.concat()
    }

    fn text(&self) -> String {
        self.template.replace("{s}", &self.secret())
    }

    /// Only the title rule strips URL queries, fragments and token paths.
    fn needs_title_rule(&self) -> bool {
        self.titles_only
            || self.kind.starts_with("url_query")
            || self.kind.starts_with("url_fragment")
            || self.kind.starts_with("url_path")
    }
}

fn corpus() -> Corpus {
    serde_json::from_str(CORPUS_JSON).expect("corpus parses")
}

#[test]
fn corpus_has_at_least_100_secrets() {
    assert!(corpus().items.len() >= 100);
}

#[test]
fn no_corpus_secret_survives_title_redaction() {
    let mut survivors = Vec::new();
    for item in corpus().items {
        let out = redact_title(&item.text());
        if out.contains(&item.secret()) {
            survivors.push(format!("{}: {out}", item.kind));
        }
    }
    assert!(
        survivors.is_empty(),
        "{} survived:\n{}",
        survivors.len(),
        survivors.join("\n")
    );
}

#[test]
fn no_corpus_secret_survives_secret_masking() {
    let mut survivors = Vec::new();
    for item in corpus().items.iter().filter(|i| !i.needs_title_rule()) {
        let out = mask_secrets(&item.text());
        if out.contains(&item.secret()) {
            survivors.push(format!("{}: {out}", item.kind));
        }
    }
    assert!(
        survivors.is_empty(),
        "{} survived:\n{}",
        survivors.len(),
        survivors.join("\n")
    );
}

#[test]
fn benign_titles_are_unchanged() {
    for title in corpus().benign_titles {
        assert_eq!(redact_title(&title), title);
    }
}

#[test]
fn keeps_keys_and_url_shape() {
    assert_eq!(
        mask_secrets("DB_PASSWORD=Winter2026 npm start"),
        "DB_PASSWORD=[redacted] npm start"
    );
    assert_eq!(
        mask_secrets("postgres://admin:s3cretPass@db:5432/app"),
        "postgres://[redacted]@db:5432/app"
    );
    assert_eq!(
        redact_title("Reset — https://app.example.com/reset?token=abc123XYZ&u=1 — Chrome"),
        "Reset — https://app.example.com/reset — Chrome"
    );
    assert_eq!(
        redact_title("Mail from dev@example.test"),
        "Mail from [email]"
    );
    assert_eq!(
        redact_title("See https://docs.rs/regex."),
        "See https://docs.rs/regex."
    );
}

#[test]
fn titles_are_truncated() {
    let out = redact_title(&"word ".repeat(100));
    assert_eq!(out.chars().count(), MAX_TITLE);
    assert!(out.ends_with('…'));
    assert_eq!(redact_title("short"), "short");
}

#[test]
fn redact_applies_per_field_rules() {
    let token = ["ghp_", "abcdefghijklmnop", "qrstuvwxyz0123456789"].concat();
    let mut focus = Event::new(
        Source::new("agent", "0.1.0"),
        "2026-11-20T09:00:00Z".parse().unwrap(),
        Some("2026-11-20T09:01:00Z".parse().unwrap()),
        Payload::WindowFocus(WindowFocus {
            app: Some("WindowsTerminal.exe".into()),
            title: Some(format!("export GITHUB_TOKEN={token} — dev@example.test")),
        }),
    );
    focus.repository = Some(RepositoryRef {
        root_path: Some("/home/dev/api".into()),
        remote_url: Some(format!("https://{token}@github.com/acme/api.git")),
    });
    redact(&mut focus);
    let json = serde_json::to_string(&focus).unwrap();
    assert!(!json.contains(&token), "{json}");
    assert!(!json.contains("dev@example.test"));
    let repo = focus.repository.as_ref().unwrap();
    assert_eq!(repo.remote_url.as_deref(), Some("github.com/acme/api"));
    assert_eq!(repo.root_path.as_deref(), Some("/home/dev/api"));
    match &focus.payload {
        Payload::WindowFocus(m) => assert_eq!(m.app.as_deref(), Some("WindowsTerminal.exe")),
        other => panic!("{other:?}"),
    }

    let mut commit = Event::new(
        Source::new("agent", "0.1.0"),
        "2026-11-20T09:00:00Z".parse().unwrap(),
        None,
        Payload::GitCommit(GitCommit {
            hash: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".into(),
            branch: Some("feature/login".into()),
            files_changed: Some(3),
            remote_url: Some("git@github.com:acme/api.git".into()),
        }),
    );
    let before = commit.clone();
    redact(&mut commit);
    match (&commit.payload, &before.payload) {
        (Payload::GitCommit(after), Payload::GitCommit(was)) => {
            assert_eq!(after.hash, was.hash, "structured fields are untouched");
            assert_eq!(after.branch, was.branch);
            assert_eq!(after.remote_url.as_deref(), Some("github.com/acme/api"));
        }
        _ => unreachable!(),
    }

    let mut workspace = Event::new(
        Source::new("vscode", "0.1.0"),
        "2026-11-20T09:00:00Z".parse().unwrap(),
        None,
        Payload::EditorWorkspaceOpen(EditorWorkspaceOpen {
            workspace_path: "/home/dev/api".into(),
            remote_url: Some("/srv/git/api.git".into()),
        }),
    );
    redact(&mut workspace);
    match &workspace.payload {
        Payload::EditorWorkspaceOpen(m) => {
            assert_eq!(
                m.remote_url.as_deref(),
                Some("/srv/git/api.git"),
                "local remotes stay"
            );
        }
        _ => unreachable!(),
    }

    let mut checkout = Event::new(
        Source::new("agent", "0.1.0"),
        "2026-11-20T09:00:00Z".parse().unwrap(),
        None,
        Payload::GitCheckout(GitCheckout {
            from_branch: Some("main".into()),
            to_branch: Some(format!("fix-{token}")),
        }),
    );
    redact(&mut checkout);
    assert!(!serde_json::to_string(&checkout).unwrap().contains(&token));

    let mut presence = Event::new(
        Source::new("agent", "0.1.0"),
        "2026-11-20T09:00:00Z".parse().unwrap(),
        Some("2026-11-20T09:01:00Z".parse().unwrap()),
        Payload::PresenceActive(Empty {}),
    );
    let before = presence.clone();
    redact(&mut presence);
    assert_eq!(presence, before);
}
