//! URL stripping and Git remote normalization (SHARED plan §4.2–4.3, SH-6).
//!
//! - [`strip_url`] keeps scheme, host, port and path, drops userinfo, query
//!   and fragment; it replaces token-like path segments with `:id`.
//! - [`normalize_remote`] makes `git@github.com:acme/api.git`,
//!   `https://github.com/acme/api` and `ssh://git@github.com/acme/api.git`
//!   all `github.com/acme/api`, the key used to match repositories
//!   to shared projects.

use url::Url;

/// Placeholder for token-like path segments.
pub const ID: &str = ":id";

/// A stripped URL, or `None` when `raw` is not an absolute URL with a host.
pub fn strip_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;
    let host = url.host_str()?;
    let mut out = format!("{}://{}", url.scheme(), host.to_ascii_lowercase());
    if let Some(port) = url.port() {
        out.push_str(&format!(":{port}"));
    }
    let segments: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
    if segments.iter().all(|s| s.is_empty()) {
        out.push('/');
        return Some(out);
    }
    for segment in segments {
        out.push('/');
        out.push_str(if looks_like_token(segment) {
            ID
        } else {
            segment
        });
    }
    Some(out)
}

/// `host/owner/repo` for a Git remote, or `None` for local paths and
/// anything unrecognizable. Credentials, ports, `.git` and trailing slashes
/// are dropped; the host is lowercased, the path keeps its case.
pub fn normalize_remote(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (host, path) = if let Ok(url) = Url::parse(raw)
        && url.has_host()
    {
        match url.scheme() {
            "http" | "https" | "ssh" | "git" | "git+ssh" | "ssh+git" => {}
            _ => return None,
        }
        (url.host_str()?.to_owned(), url.path().to_owned())
    } else {
        // scp-like: [user@]host:path, but not a Windows drive (C:\x) or a
        // local path.
        let (user_host, path) = raw.split_once(':')?;
        let host = user_host.rsplit('@').next()?;
        let local = host.len() <= 1
            || host.contains(['/', '\\'])
            || path.starts_with("//")
            || raw.starts_with(['/', '.', '~']);
        if local || host.is_empty() {
            return None;
        }
        (host.to_owned(), path.to_owned())
    };

    let host = host.to_ascii_lowercase();
    let path = path.trim_matches('/');
    let path = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');
    // Azure DevOps SSH remotes carry an API version segment.
    let path = path.strip_prefix("v3/").unwrap_or(path);
    if host.is_empty() || path.is_empty() || path.split('/').any(str::is_empty) {
        return None;
    }
    Some(format!("{host}/{path}"))
}

/// Long hex, UUIDs, and long mixed alphanumeric strings (base64/base62
/// tokens). Plain words, slugs and short numbers stay.
fn looks_like_token(segment: &str) -> bool {
    let s = segment;
    let len = s.chars().count();
    if len >= 16 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return s.chars().filter(char::is_ascii_digit).count() >= 2;
    }
    if len >= 24 {
        let token_chars = s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '=' | '.' | '~' | '%'));
        let digits = s.chars().filter(char::is_ascii_digit).count();
        let upper = s.chars().filter(char::is_ascii_uppercase).count();
        let lower = s.chars().filter(char::is_ascii_lowercase).count();
        // Slugs are mostly lowercase words separated by dashes.
        let dashes = s.chars().filter(|&c| c == '-').count();
        let sluggy = upper == 0 && dashes * 12 >= len;
        return token_chars && !sluggy && digits >= 2 && (upper > 0 || lower > 0);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_urls() {
        let cases = [
            (
                "https://user:pa55@Example.COM:8443/a/b?token=abc#frag",
                Some("https://example.com:8443/a/b"),
            ),
            (
                "https://github.com/acme/api/pull/42",
                Some("https://github.com/acme/api/pull/42"),
            ),
            ("https://docs.rs/", Some("https://docs.rs/")),
            ("https://docs.rs", Some("https://docs.rs/")),
            (
                "http://localhost:3000/login?next=/x",
                Some("http://localhost:3000/login"),
            ),
            (
                "https://app.example.com/reset/9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
                Some("https://app.example.com/reset/:id"),
            ),
            (
                "https://example.com/files/123e4567-e89b-12d3-a456-426614174000/view",
                Some("https://example.com/files/:id/view"),
            ),
            (
                "https://example.com/invite/Xy7Qp2Lm9Rt4Vb8Nc3Kd6Wf1",
                Some("https://example.com/invite/:id"),
            ),
            (
                "https://blog.example.com/how-to-write-a-rust-macro-2024-edition",
                Some("https://blog.example.com/how-to-write-a-rust-macro-2024-edition"),
            ),
            (
                "https://example.com/users/123456",
                Some("https://example.com/users/123456"),
            ),
            ("not a url", None),
            ("mailto:dev@example.test", None),
            ("file:///home/dev/notes.txt", None),
            ("", None),
        ];
        for (raw, expected) in cases {
            assert_eq!(strip_url(raw).as_deref(), expected, "{raw}");
        }
    }

    #[test]
    fn normalizes_remotes() {
        let cases = [
            ("git@github.com:acme/api.git", Some("github.com/acme/api")),
            ("https://github.com/acme/api", Some("github.com/acme/api")),
            (
                "https://github.com/acme/api.git",
                Some("github.com/acme/api"),
            ),
            ("https://github.com/acme/api/", Some("github.com/acme/api")),
            (
                "ssh://git@github.com/acme/api.git",
                Some("github.com/acme/api"),
            ),
            (
                "ssh://git@GitHub.com:22/acme/api.git",
                Some("github.com/acme/api"),
            ),
            (
                "https://token:x-oauth@github.com/acme/api.git",
                Some("github.com/acme/api"),
            ),
            (
                "https://ghp_secret@github.com/acme/api",
                Some("github.com/acme/api"),
            ),
            (
                "git://git.example.org/proj.git",
                Some("git.example.org/proj"),
            ),
            (
                "gitlab.com:group/sub/proj.git",
                Some("gitlab.com/group/sub/proj"),
            ),
            (
                "https://gitlab.com/group/subgroup/Proj.git",
                Some("gitlab.com/group/subgroup/Proj"),
            ),
            (
                "git@ssh.dev.azure.com:v3/org/project/repo",
                Some("ssh.dev.azure.com/org/project/repo"),
            ),
            (
                "https://dev.azure.com/org/project/_git/repo",
                Some("dev.azure.com/org/project/_git/repo"),
            ),
            (
                "  git@github.com:acme/api.git\n",
                Some("github.com/acme/api"),
            ),
            ("/home/dev/repos/api.git", None),
            ("../api", None),
            ("C:\\repos\\api", None),
            ("C:/repos/api", None),
            ("file:///srv/git/api.git", None),
            ("https://github.com", None),
            ("", None),
        ];
        for (raw, expected) in cases {
            assert_eq!(normalize_remote(raw).as_deref(), expected, "{raw:?}");
        }
    }
}
