//! Default secret patterns (SHARED plan §4.2). Rule set version:
//! [`crate::config::REDACTION_VERSION`]; bump it when the rules change.
//!
//! Order matters: specific token formats first, then the generic
//! `KEY=value` and high-entropy rules that catch the rest.

use std::sync::LazyLock;

use regex::{Captures, Regex};

/// What a masked secret becomes.
pub const MASK: &str = "[redacted]";
/// What a masked e-mail address becomes.
pub const EMAIL_MASK: &str = "[email]";

struct Rule {
    regex: Regex,
    /// `None`: replace the whole match with [`MASK`]; `Some(f)`: custom.
    replace: Option<fn(&Captures) -> String>,
}

fn rule(pattern: &str) -> Rule {
    Rule {
        regex: Regex::new(pattern).expect("valid secret pattern"),
        replace: None,
    }
}

fn rule_with(pattern: &str, replace: fn(&Captures) -> String) -> Rule {
    Rule {
        regex: Regex::new(pattern).expect("valid secret pattern"),
        replace: Some(replace),
    }
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        // PEM private keys, with or without the body and END line.
        rule(
            r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----(?:[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|[A-Za-z0-9+/=\s]*)",
        ),
        // Credentials in URLs: scheme://user:pass@host → scheme://[redacted]@host.
        rule_with(
            r"(?i)\b([a-z][a-z0-9+.-]{1,20}://)[^\s/@:]*:[^\s/@]+@",
            |c| format!("{}{MASK}@", &c[1]),
        ),
        // Token-only userinfo: https://ghp_xxx@github.com.
        rule_with(r"(?i)\b(https?://)[A-Za-z0-9_\-.~]{16,}@", |c| {
            format!("{}{MASK}@", &c[1])
        }),
        // JWTs (three base64url parts, header starts with `{"`).
        rule(r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}"),
        // Provider tokens.
        rule(r"\b(?:AKIA|ASIA|ABIA|ACCA|AGPA|AIDA|AROA|ANPA|ANVA|AIPA)[0-9A-Z]{16}\b"),
        rule(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{30,}\b"),
        rule(r"\bgithub_pat_[A-Za-z0-9_]{22,}"),
        rule(r"\bglpat-[A-Za-z0-9_\-]{20,}"),
        rule(r"\bgl(?:dt|rt|ptt|ft|imt|agent|oas|cbt|soat)-[A-Za-z0-9_\-]{20,}"),
        rule(r"\bxox[abposre]-[A-Za-z0-9-]{10,}"),
        rule(r"https://hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9/_\-]+"),
        rule(r"https://(?:[a-z0-9-]+\.)?discord(?:app)?\.com/api/webhooks/[0-9]+/[A-Za-z0-9_\-]+"),
        rule(r"\b(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{16,}"),
        rule(r"\bwhsec_[A-Za-z0-9]{20,}"),
        rule(r"\bAIza[0-9A-Za-z_\-]{35}"),
        rule(r"\bya29\.[0-9A-Za-z_\-]{20,}"),
        rule(r"\bsk-(?:ant-|proj-|svcacct-)?[A-Za-z0-9_\-]{20,}"),
        rule(r"\bnpm_[A-Za-z0-9]{36}\b"),
        rule(r"\bpypi-[A-Za-z0-9_\-]{50,}"),
        rule(r"\bSG\.[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}"),
        rule(r"\b(?:AC|SK)[0-9a-f]{32}\b"),
        rule(r"\bdop_v1_[a-f0-9]{64}\b"),
        rule(r"\bshp(?:at|ca|pa|ss)_[a-fA-F0-9]{32}\b"),
        rule(r"\bhf_[A-Za-z0-9]{30,}\b"),
        rule(r"\b(?:EAA[A-Za-z0-9]{30,})"),
        rule(r"\bAccountKey=[A-Za-z0-9+/=]{20,}"),
        rule(r"\bSharedAccessSignature=[^\s;]+|\bsig=[A-Za-z0-9%+/=]{20,}"),
        // Authorization headers.
        rule_with(
            r"(?i)\b(bearer|token|basic|digest)\s+[A-Za-z0-9\-._~+/]{12,}=*",
            |c| format!("{} {MASK}", &c[1]),
        ),
        // KEY=value / KEY: value where KEY ends in a secret word (API_KEY,
        // DB_PASSWORD, client_secret, x-auth-token). Keeps the key.
        rule_with(
            r#"(?i)\b([A-Za-z0-9_.\-]*(?:key|keys|token|tokens|secret|secrets|passw(?:or)?d|passphrase|pwd|credentials?|auth|cookie|signature))\b(\s*[=:]\s*|\s+)("[^"\n]*"|'[^'\n]*'|[^\s"',;&]+)"#,
            |c| {
                let value = &c[3];
                // `--password` style flags followed by a word: mask only
                // when the separator is `=`/`:` or the value looks secret.
                if c[2].trim().is_empty() && !looks_secret(value) {
                    return c[0].to_owned();
                }
                format!("{}{}{MASK}", &c[1], &c[2])
            },
        ),
        // Generic high-entropy strings: long hex / base64 that slipped past.
        rule_with(r"[A-Za-z0-9+_\-]{32,}={0,2}", |c| {
            let s = &c[0];
            if looks_secret(s) {
                MASK.to_owned()
            } else {
                s.to_owned()
            }
        }),
    ]
});

static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b").expect("valid e-mail pattern")
});

/// Masks every secret pattern in `text`, including the generic
/// high-entropy rule (the `full` level).
pub fn mask_secrets(text: &str) -> String {
    apply(text, &RULES)
}

/// Masks credentials only: every specific pattern (keys, tokens, passwords,
/// private keys, URL user:password) but not the generic high-entropy rule,
/// so ids, hashes and folder names stay (the `secrets` level, D18).
pub fn mask_credentials(text: &str) -> String {
    apply(text, &RULES[..RULES.len() - 1])
}

fn apply(text: &str, rules: &[Rule]) -> String {
    let mut out = text.to_owned();
    for rule in rules {
        if !rule.regex.is_match(&out) {
            continue;
        }
        out = match rule.replace {
            None => rule.regex.replace_all(&out, MASK).into_owned(),
            Some(f) => rule
                .regex
                .replace_all(&out, |c: &Captures| f(c))
                .into_owned(),
        };
    }
    out
}

/// Masks e-mail addresses.
pub fn mask_emails(text: &str) -> String {
    EMAIL.replace_all(text, EMAIL_MASK).into_owned()
}

/// A token, not a word: long enough, with digits and letters mixed, and
/// not just a long lowercase identifier.
fn looks_secret(value: &str) -> bool {
    let v = value.trim_matches(['"', '\'']);
    if v.len() < 16 || v == MASK {
        return false;
    }
    let digits = v.chars().filter(char::is_ascii_digit).count();
    let upper = v.chars().filter(char::is_ascii_uppercase).count();
    let lower = v.chars().filter(char::is_ascii_lowercase).count();
    let hex = v.chars().all(|c| c.is_ascii_hexdigit());
    // 40-hex Git hashes in free text are fine to lose; over-masking is safe.
    (hex && v.len() >= 32 && digits >= 4)
        || (digits >= 3 && (upper >= 2 || lower >= 2) && upper + lower >= 6)
}
