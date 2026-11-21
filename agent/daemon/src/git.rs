//! Git tracker (AG-10): finds repositories under the project roots and turns
//! new reflog entries into `git.commit`, `git.checkout` and `git.merge` events.
//!
//! The reflog (`<git dir>/logs/HEAD`) has one line per HEAD change with its
//! exact time, so watching means reading the lines appended since the last
//! poll. Polling (every [`POLL_EVERY`]) is a `stat` per repository; there is
//! no file watcher and no `git` process. A repository seen for the first time
//! starts at the end of its reflog: history is the backfill's job (AG-11).
//!
//! Only metadata leaves the repository: commit hash, branch names, the
//! repository root and its remote. Never commit messages, diffs or file
//! contents. Events go through the pipeline (exclusions, redaction).

use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone, Utc};
use secretary_shared::event::{GitCheckout, GitCommit, GitMerge, Payload};
use secretary_shared::{Event, RepositoryRef};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::buffer::agent_source;
use crate::pipeline::Pipeline;
use crate::status::StatusBoard;

/// Acceptance: commits and checkouts appear within 10 s.
pub const POLL_EVERY: Duration = Duration::from_secs(5);
/// Roots are rescanned this often for new or removed repositories.
pub const DISCOVER_EVERY: Duration = Duration::from_secs(600);
/// How deep below a root repositories are looked for.
pub const MAX_DEPTH: usize = 4;
/// Folders never descended into while looking for repositories.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "vendor",
    "venv",
    ".venv",
    "__pycache__",
    "AppData",
    "Library",
    "Applications",
    "Program Files",
    "Program Files (x86)",
    "Windows",
    "$Recycle.Bin",
    "OneDrive",
    "snap",
];

/// A working tree and where its Git data lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub worktree: PathBuf,
    /// `.git` directory (for a linked worktree: its private git dir).
    pub git_dir: PathBuf,
    /// Shared git dir (`commondir`), where `config` lives.
    pub common_dir: PathBuf,
}

impl Repo {
    /// `dir` is a working tree when it has a `.git` directory or a `.git`
    /// file (`gitdir: …`, used by worktrees and submodules).
    pub fn open(dir: &Path) -> Option<Self> {
        let dot_git = dir.join(".git");
        let git_dir = if dot_git.is_dir() {
            dot_git
        } else if dot_git.is_file() {
            let text = std::fs::read_to_string(&dot_git).ok()?;
            let target = text.trim().strip_prefix("gitdir:")?.trim();
            let target = Path::new(target);
            if target.is_absolute() {
                target.to_owned()
            } else {
                dir.join(target)
            }
        } else {
            return None;
        };
        if !git_dir.join("HEAD").is_file() {
            return None;
        }
        let common_dir = std::fs::read_to_string(git_dir.join("commondir"))
            .ok()
            .map(|c| {
                let c = Path::new(c.trim());
                if c.is_absolute() {
                    c.to_owned()
                } else {
                    git_dir.join(c)
                }
            })
            .unwrap_or_else(|| git_dir.clone());
        Some(Self {
            worktree: dir.to_owned(),
            git_dir,
            common_dir,
        })
    }

    pub fn reflog(&self) -> PathBuf {
        self.git_dir.join("logs").join("HEAD")
    }

    /// The checked-out branch, or `None` on a detached HEAD.
    pub fn branch(&self) -> Option<String> {
        let head = std::fs::read_to_string(self.git_dir.join("HEAD")).ok()?;
        head.trim()
            .strip_prefix("ref: refs/heads/")
            .map(str::to_owned)
    }

    /// URL of `origin`, else of the first remote in `config`.
    pub fn remote_url(&self) -> Option<String> {
        let config = std::fs::read_to_string(self.common_dir.join("config")).ok()?;
        remote_url_from_config(&config)
    }
}

pub fn remote_url_from_config(config: &str) -> Option<String> {
    let mut section: Option<String> = None;
    let mut first = None;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line
                .strip_prefix("[remote \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
                .map(str::to_owned);
            continue;
        }
        let Some(name) = &section else { continue };
        if let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("url")
        {
            let url = value.trim().to_owned();
            if name == "origin" {
                return Some(url);
            }
            first.get_or_insert(url);
        }
    }
    first
}

/// Repositories under `roots`, at most [`MAX_DEPTH`] levels down. Does not
/// descend into a repository it found, hidden folders, or [`SKIP_DIRS`].
pub fn discover(roots: &[PathBuf]) -> Vec<Repo> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        walk(root, 0, &mut found, &mut seen);
    }
    found
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<Repo>, seen: &mut HashSet<PathBuf>) {
    if !seen.insert(dir.to_owned()) {
        return;
    }
    if let Some(repo) = Repo::open(dir) {
        found.push(repo);
        return;
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            continue;
        }
        walk(&entry.path(), depth + 1, found, seen);
    }
}

/// One reflog line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    pub new: String,
    pub at: DateTime<Utc>,
    pub message: String,
}

/// `<old> <new> <name> <<email>> <unix seconds> <±hhmm>\t<message>`.
pub fn parse_reflog_line(line: &str) -> Option<ReflogEntry> {
    let (head, message) = line.split_once('\t')?;
    let mut parts = head.split(' ');
    let _old = parts.next()?;
    let new = parts.next()?;
    let mut tail = head.rsplitn(3, ' ');
    let _tz = tail.next()?;
    let seconds: i64 = tail.next()?.parse().ok()?;
    if new.len() < 7 || !new.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(ReflogEntry {
        new: new.to_owned(),
        at: Utc.timestamp_opt(seconds, 0).single()?,
        message: message.trim_end().to_owned(),
    })
}

/// The event for a reflog entry, or `None` for moves that are not work
/// (resets, rebase steps, pulls, branch renames).
pub fn classify(
    entry: &ReflogEntry,
    branch: Option<&str>,
    remote: Option<&str>,
) -> Option<Payload> {
    let msg = entry.message.as_str();
    let branch = branch.map(str::to_owned);
    if msg.starts_with("commit (merge):") {
        return Some(Payload::GitMerge(GitMerge { branch }));
    }
    if msg.starts_with("commit:")
        || msg.starts_with("commit (initial):")
        || msg.starts_with("commit (amend):")
        || msg.starts_with("cherry-pick:")
        || msg.starts_with("revert:")
    {
        return Some(Payload::GitCommit(GitCommit {
            hash: entry.new.clone(),
            branch,
            files_changed: None,
            remote_url: remote.map(str::to_owned),
        }));
    }
    if let Some(moving) = msg.strip_prefix("checkout: moving from ") {
        let (from, to) = moving.split_once(" to ")?;
        if from == to {
            return None;
        }
        return Some(Payload::GitCheckout(GitCheckout {
            from_branch: Some(from.to_owned()),
            to_branch: Some(to.to_owned()),
        }));
    }
    if let Some(rest) = msg.strip_prefix("merge ") {
        let merged = rest.split(':').next().unwrap_or_default().trim();
        return Some(Payload::GitMerge(GitMerge {
            branch: (!merged.is_empty()).then(|| merged.to_owned()),
        }));
    }
    None
}

/// Reflog read positions, persisted so restarts never re-report or skip.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Offsets {
    repos: BTreeMap<String, u64>,
}

pub struct Tracker {
    state_file: PathBuf,
    offsets: Offsets,
    repos: Vec<Repo>,
    dirty: bool,
}

impl Tracker {
    pub fn load(state_file: &Path) -> Self {
        let offsets = std::fs::read(state_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            state_file: state_file.to_owned(),
            offsets,
            repos: Vec::new(),
            dirty: false,
        }
    }

    pub fn repos(&self) -> &[Repo] {
        &self.repos
    }

    /// Replaces the watched set. New repositories start at their reflog's end.
    pub fn set_repos(&mut self, repos: Vec<Repo>) {
        for repo in &repos {
            if let std::collections::btree_map::Entry::Vacant(slot) =
                self.offsets.repos.entry(key(repo))
            {
                slot.insert(std::fs::metadata(repo.reflog()).map_or(0, |m| m.len()));
                self.dirty = true;
            }
        }
        let keep: HashSet<String> = repos.iter().map(key).collect();
        let before = self.offsets.repos.len();
        self.offsets.repos.retain(|k, _| keep.contains(k));
        self.dirty |= self.offsets.repos.len() != before;
        self.repos = repos;
    }

    /// Events for every complete reflog line appended since the last poll.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        for repo in &self.repos {
            let key = key(repo);
            let offset = self.offsets.repos.get(&key).copied().unwrap_or(0);
            let Ok(len) = std::fs::metadata(repo.reflog()).map(|m| m.len()) else {
                continue;
            };
            if len == offset {
                continue;
            }
            if len < offset {
                // Expired or rewritten reflog: start over at its end.
                self.offsets.repos.insert(key, len);
                self.dirty = true;
                continue;
            }
            let Some((lines, consumed)) = read_complete_lines(&repo.reflog(), offset, len) else {
                continue;
            };
            let (branch, remote) = (repo.branch(), repo.remote_url());
            for line in lines.lines() {
                let Some(entry) = parse_reflog_line(line) else {
                    continue;
                };
                if let Some(payload) = classify(&entry, branch.as_deref(), remote.as_deref()) {
                    let mut event = Event::new(agent_source(), entry.at, None, payload);
                    event.repository = Some(RepositoryRef {
                        root_path: Some(repo.worktree.to_string_lossy().into_owned()),
                        remote_url: remote.clone(),
                    });
                    events.push(event);
                }
            }
            self.offsets.repos.insert(key, offset + consumed);
            self.dirty = true;
        }
        events
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let tmp = self.state_file.with_extension("json.tmp");
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.offsets)
            && std::fs::write(&tmp, bytes).is_ok()
            && std::fs::rename(&tmp, &self.state_file).is_ok()
        {
            self.dirty = false;
        }
    }
}

fn key(repo: &Repo) -> String {
    repo.git_dir.to_string_lossy().into_owned()
}

/// Bytes `offset..len` up to the last newline (a line being written now is
/// left for the next poll), and how many bytes that is.
fn read_complete_lines(path: &Path, offset: u64, len: u64) -> Option<(String, u64)> {
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::with_capacity((len - offset) as usize);
    file.take(len - offset).read_to_end(&mut bytes).ok()?;
    let end = bytes.iter().rposition(|&b| b == b'\n')? + 1;
    bytes.truncate(end);
    Some((String::from_utf8_lossy(&bytes).into_owned(), end as u64))
}

/// Runs the tracker on a blocking thread until cancelled. `roots` is called
/// at every discovery, so config changes apply at the next rescan.
pub async fn run(
    pipeline: Arc<Pipeline>,
    status: Arc<StatusBoard>,
    state_file: PathBuf,
    roots: impl Fn() -> Vec<PathBuf> + Send + 'static,
    token: CancellationToken,
) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut tracker = Tracker::load(&state_file);
        let mut discovered_at: Option<Instant> = None;
        while !token.is_cancelled() {
            if discovered_at.is_none_or(|at| at.elapsed() >= DISCOVER_EVERY) {
                let roots = roots();
                let repos = discover(&roots);
                info!(
                    roots = roots.len(),
                    repositories = repos.len(),
                    "git repositories discovered"
                );
                tracker.set_repos(repos);
                status.update(|s| s.git.repositories = tracker.repos().len());
                discovered_at = Some(Instant::now());
            }
            let events = tracker.poll();
            if !events.is_empty() {
                debug!(events = events.len(), "git activity");
                status.update(|s| s.git.last_event_at = Some(Utc::now()));
            }
            for event in events {
                pipeline.submit(event);
            }
            tracker.save();
            sleep_unless_cancelled(&token, POLL_EVERY);
        }
        tracker.save();
    })
    .await
    .map_err(|err| {
        warn!(error = %err, "git tracker stopped unexpectedly");
        anyhow::anyhow!(err)
    })
}

fn sleep_unless_cancelled(token: &CancellationToken, total: Duration) {
    let step = Duration::from_millis(100);
    let mut slept = Duration::ZERO;
    while slept < total && !token.is_cancelled() {
        std::thread::sleep(step);
        slept += step;
    }
}

/// The roots to scan: configured ones (`~` expanded), else the home folder.
pub fn effective_roots(configured: &[String]) -> Vec<PathBuf> {
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_owned());
    let expand = |p: &String| match (&home, p.strip_prefix('~')) {
        (Some(home), Some(rest)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(p),
    };
    let roots: Vec<PathBuf> = configured
        .iter()
        .filter(|p| !p.trim().is_empty())
        .map(expand)
        .collect();
    if roots.is_empty() {
        home.into_iter().collect()
    } else {
        roots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(new: &str, seconds: i64, message: &str) -> String {
        format!(
            "{} {new} Dev <dev@example.test> {seconds} +0100\t{message}\n",
            "0".repeat(40)
        )
    }

    /// A minimal `.git` layout without running git.
    fn fake_repo(dir: &Path, branch: &str, remote: Option<&str>) -> Repo {
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("logs")).unwrap();
        std::fs::write(git.join("HEAD"), format!("ref: refs/heads/{branch}\n")).unwrap();
        let mut config = "[core]\n\tbare = false\n".to_owned();
        if let Some(url) = remote {
            config.push_str(&format!("[remote \"origin\"]\n\turl = {url}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n"));
        }
        std::fs::write(git.join("config"), config).unwrap();
        std::fs::write(
            git.join("logs/HEAD"),
            line(&"a".repeat(40), 1_700_000_000, "commit (initial): first"),
        )
        .unwrap();
        Repo::open(dir).unwrap()
    }

    fn append(repo: &Repo, text: &str) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(repo.reflog())
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn parses_real_reflog_lines() {
        let entry = parse_reflog_line(
            "0000000000000000000000000000000000000000 f6d22db61378bd393c01a1f00cb501aae3838d6b AbdoPrDZ <a@b.c> 1794053957 +0100\tcommit (initial): repo: initial project setup",
        )
        .unwrap();
        assert_eq!(entry.new, "f6d22db61378bd393c01a1f00cb501aae3838d6b");
        assert_eq!(entry.at.timestamp(), 1_794_053_957);
        assert_eq!(
            entry.message,
            "commit (initial): repo: initial project setup"
        );
        // Names with spaces, and garbage.
        assert!(
            parse_reflog_line(&line(&"b".repeat(40), 5, "commit: x").replace("Dev", "Dev Eloper"))
                .is_some()
        );
        assert_eq!(parse_reflog_line("not a reflog line"), None);
        assert_eq!(parse_reflog_line("x y z w\tmsg"), None);
    }

    #[test]
    fn classifies_work_and_ignores_the_rest() {
        let entry = |m: &str| ReflogEntry {
            new: "c".repeat(40),
            at: Utc::now(),
            message: m.into(),
        };
        let kind = |m: &str| {
            classify(&entry(m), Some("main"), Some("github.com/acme/api")).map(|p| p.event_type())
        };
        for m in [
            "commit: fix",
            "commit (initial): x",
            "commit (amend): x",
            "cherry-pick: x",
            "revert: x",
        ] {
            assert_eq!(kind(m), Some("git.commit"), "{m}");
        }
        assert_eq!(
            kind("commit (merge): Merge branch 'feature'"),
            Some("git.merge")
        );
        assert_eq!(kind("merge feature: Fast-forward"), Some("git.merge"));
        assert_eq!(
            kind("checkout: moving from main to feature"),
            Some("git.checkout")
        );
        for m in [
            "checkout: moving from main to main",
            "reset: moving to HEAD~1",
            "rebase (finish): returning to refs/heads/main",
            "pull: Fast-forward",
            "Branch: renamed refs/heads/main to refs/heads/main",
        ] {
            assert_eq!(kind(m), None, "{m}");
        }
        match classify(
            &entry("checkout: moving from main to feature/x"),
            None,
            None,
        ) {
            Some(Payload::GitCheckout(c)) => {
                assert_eq!(c.from_branch.as_deref(), Some("main"));
                assert_eq!(c.to_branch.as_deref(), Some("feature/x"));
            }
            other => panic!("{other:?}"),
        }
        match classify(
            &entry("commit: x"),
            Some("main"),
            Some("git@github.com:acme/api.git"),
        ) {
            Some(Payload::GitCommit(c)) => {
                assert_eq!(c.hash, "c".repeat(40));
                assert_eq!(c.branch.as_deref(), Some("main"));
                assert_eq!(c.remote_url.as_deref(), Some("git@github.com:acme/api.git"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reads_the_origin_remote() {
        let config = "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = https://x/up.git\n[remote \"origin\"]\n\turl = git@github.com:acme/api.git\n";
        assert_eq!(
            remote_url_from_config(config).as_deref(),
            Some("git@github.com:acme/api.git")
        );
        let only = "[remote \"fork\"]\n  url = https://x/fork.git\n";
        assert_eq!(
            remote_url_from_config(only).as_deref(),
            Some("https://x/fork.git")
        );
        assert_eq!(remote_url_from_config("[core]\n"), None);
    }

    #[test]
    fn discovers_repositories_but_not_inside_them_or_skipped_dirs() {
        let root = tempfile::tempdir().unwrap();
        fake_repo(&root.path().join("work/api"), "main", None);
        fake_repo(&root.path().join("work/api/vendor/lib"), "main", None); // inside a repo
        fake_repo(&root.path().join("node_modules/pkg"), "main", None);
        fake_repo(&root.path().join(".hidden/repo"), "main", None);
        fake_repo(&root.path().join("a/b/c/d/too-deep"), "main", None);
        fake_repo(&root.path().join("web"), "main", None);
        let mut found: Vec<String> = discover(&[root.path().to_owned()])
            .iter()
            .map(|r| {
                r.worktree
                    .strip_prefix(root.path())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        found.sort();
        assert_eq!(found, ["web", "work/api"]);
    }

    #[test]
    fn worktree_git_file_points_to_the_real_git_dir() {
        let root = tempfile::tempdir().unwrap();
        let main = fake_repo(
            &root.path().join("main"),
            "main",
            Some("https://github.com/acme/api.git"),
        );
        let private = main.git_dir.join("worktrees/wt");
        std::fs::create_dir_all(private.join("logs")).unwrap();
        std::fs::write(private.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
        std::fs::write(private.join("commondir"), "../..\n").unwrap();
        let wt = root.path().join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", private.display())).unwrap();

        let repo = Repo::open(&wt).unwrap();
        assert_eq!(repo.branch().as_deref(), Some("feature"));
        assert_eq!(
            repo.remote_url().as_deref(),
            Some("https://github.com/acme/api.git")
        );
    }

    #[test]
    fn reports_only_new_lines_once_across_restarts() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("git_state.json");
        let repo = fake_repo(
            &root.path().join("api"),
            "main",
            Some("git@github.com:acme/api.git"),
        );

        let mut tracker = Tracker::load(&state);
        tracker.set_repos(vec![repo.clone()]);
        assert!(tracker.poll().is_empty(), "history is not reported");

        append(&repo, &line(&"b".repeat(40), 1_700_000_100, "commit: work"));
        append(
            &repo,
            &line(
                &"b".repeat(40),
                1_700_000_200,
                "checkout: moving from main to feature",
            ),
        );
        append(&repo, "0000 partial line being writ"); // no newline yet
        let events = tracker.poll();
        assert_eq!(
            events.iter().map(Event::event_type).collect::<Vec<_>>(),
            ["git.commit", "git.checkout"]
        );
        assert_eq!(events[0].timestamp_start.timestamp(), 1_700_000_100);
        let repo_ref = events[0].repository.as_ref().unwrap();
        assert_eq!(
            repo_ref.remote_url.as_deref(),
            Some("git@github.com:acme/api.git")
        );
        assert!(repo_ref.root_path.as_deref().unwrap().ends_with("api"));
        tracker.save();

        // A restart resumes after the last complete line.
        let mut restarted = Tracker::load(&state);
        restarted.set_repos(vec![repo.clone()]);
        assert!(restarted.poll().is_empty());
        append(&repo, "\n"); // completes the partial line: garbage, skipped
        append(&repo, &line(&"d".repeat(40), 1_700_000_300, "commit: more"));
        // The partial line completes as garbage and is skipped; the commit counts.
        let events = restarted.poll();
        assert_eq!(
            events.iter().map(Event::event_type).collect::<Vec<_>>(),
            ["git.commit"]
        );
    }

    #[test]
    fn a_shrunk_reflog_restarts_at_its_end() {
        let root = tempfile::tempdir().unwrap();
        let repo = fake_repo(&root.path().join("api"), "main", None);
        let mut tracker = Tracker::load(&root.path().join("s.json"));
        tracker.set_repos(vec![repo.clone()]);
        append(&repo, &line(&"b".repeat(40), 1, "commit: a"));
        assert_eq!(tracker.poll().len(), 1);
        std::fs::write(repo.reflog(), "").unwrap(); // `git reflog expire`
        assert!(tracker.poll().is_empty());
        append(&repo, &line(&"e".repeat(40), 2, "commit: b"));
        assert_eq!(tracker.poll().len(), 1);
    }

    #[test]
    fn roots_default_to_home_and_expand_tilde() {
        let home = directories::BaseDirs::new().unwrap().home_dir().to_owned();
        assert_eq!(effective_roots(&[]), vec![home.clone()]);
        assert_eq!(effective_roots(&["  ".into()]), vec![home.clone()]);
        assert_eq!(
            effective_roots(&["~/code".into(), "/srv/repos".into()]),
            vec![home.join("code"), PathBuf::from("/srv/repos")]
        );
    }
}
