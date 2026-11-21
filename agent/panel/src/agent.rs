//! Talking to the agent daemon: its directories, starting and stopping it as
//! a separate background process, and running `login --json` / `logout`.
//!
//! The widget never links the daemon in: it runs the `secretary-agent`
//! executable that sits next to it, without a console window.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};

use secretary_shared::local::STATUS_FILE;

/// Same override as the daemon (`agent/daemon/src/paths.rs`).
const HOME_ENV: &str = "SECRETARY_HOME";

#[derive(Debug, Clone)]
pub struct AgentDirs {
    pub config: PathBuf,
    pub data: PathBuf,
}

impl AgentDirs {
    /// Resolved exactly as the daemon does.
    pub fn resolve() -> Self {
        if let Some(home) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            let home = PathBuf::from(home);
            return Self {
                config: home.join("config"),
                data: home.join("data"),
            };
        }
        match directories::ProjectDirs::from("", "", "secretary") {
            Some(d) => Self {
                config: d.config_dir().to_path_buf(),
                data: d.data_local_dir().to_path_buf(),
            },
            None => Self {
                config: PathBuf::from("."),
                data: PathBuf::from("."),
            },
        }
    }

    pub fn status_file(&self) -> PathBuf {
        self.data.join(STATUS_FILE)
    }

    /// `server_url` from `agent.toml`, if set.
    pub fn configured_server(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.config.join("agent.toml")).ok()?;
        let table: toml::Table = text.parse().ok()?;
        table.get("server_url")?.as_str().map(str::to_owned)
    }
}

/// `secretary-agent[.exe]` next to this program (or on PATH as a fallback).
pub fn agent_exe() -> PathBuf {
    let name = if cfg!(windows) {
        "secretary-agent.exe"
    } else {
        "secretary-agent"
    };
    std::env::current_exe()
        .ok()
        .and_then(|me| me.parent().map(|dir| dir.join(name)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// A command for the agent that opens no console window and does not die
/// with this process.
fn agent_command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0); // not stopped by a Ctrl+C aimed at the widget
    }
    cmd
}

/// Starts `secretary-agent run` in the background. A second daemon exits on
/// its own (single-instance lock), so this is safe to call when unsure.
pub fn start_daemon(exe: &Path) -> std::io::Result<()> {
    agent_command(exe)
        .arg("run")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

/// Asks the daemon to stop (`secretary-agent stop`).
pub fn stop_daemon(exe: &Path) {
    run_detached(exe, &["stop"]);
}

pub fn logout(exe: &Path) {
    run_detached(exe, &["logout"]);
}

fn run_detached(exe: &Path, args: &[&str]) {
    if let Ok(mut child) = agent_command(exe)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        // Reap it off the UI thread.
        std::thread::spawn(move || child.wait());
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoginEvent {
    Code {
        url: String,
        user_code: String,
        expires_in: u64,
    },
    SignedIn {
        email: Option<String>,
    },
    Failed(String),
}

/// A running `secretary-agent login --json`.
pub struct LoginProcess {
    child: Child,
    pub events: Receiver<LoginEvent>,
}

impl LoginProcess {
    pub fn start(exe: &Path, server: &str) -> std::io::Result<Self> {
        let mut child = agent_command(exe)
            .args(["login", "--json", "--server", server])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            let mut finished = false;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(event) = parse_line(&line) {
                    finished |= !matches!(event, LoginEvent::Code { .. });
                    if tx.send(event).is_err() {
                        return;
                    }
                }
            }
            if !finished {
                let _ = tx.send(LoginEvent::Failed("sign-in ended unexpectedly".into()));
            }
        });
        Ok(Self { child, events })
    }

    pub fn cancel(mut self) {
        let _ = self.child.kill();
        std::thread::spawn(move || self.child.wait());
    }
}

impl Drop for LoginProcess {
    fn drop(&mut self) {
        // Reaps a finished process; harmless when already waited.
        let _ = self.child.try_wait();
    }
}

pub fn parse_line(line: &str) -> Option<LoginEvent> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let text = |k: &str| value.get(k).and_then(|v| v.as_str()).map(str::to_owned);
    match value.get("event")?.as_str()? {
        "code" => Some(LoginEvent::Code {
            url: text("verification_uri_complete").or_else(|| text("verification_uri"))?,
            user_code: text("user_code")?,
            expires_in: value
                .get("expires_in")
                .and_then(|v| v.as_u64())
                .unwrap_or(600),
        }),
        "signed_in" => Some(LoginEvent::SignedIn {
            email: text("email"),
        }),
        "error" => Some(LoginEvent::Failed(
            text("message").unwrap_or_else(|| "sign-in failed".into()),
        )),
        _ => None,
    }
}

pub fn open_folder(path: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(program).arg(path).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_login_json_lines() {
        assert_eq!(
            parse_line(
                r#"{"event":"code","verification_uri":"https://kc/device","verification_uri_complete":"https://kc/device?user_code=ABCD-EFGH","user_code":"ABCD-EFGH","expires_in":600}"#
            ),
            Some(LoginEvent::Code {
                url: "https://kc/device?user_code=ABCD-EFGH".into(),
                user_code: "ABCD-EFGH".into(),
                expires_in: 600
            })
        );
        assert_eq!(
            parse_line(r#"{"event":"signed_in","email":"dev@example.test","device_id":4}"#),
            Some(LoginEvent::SignedIn {
                email: Some("dev@example.test".into())
            })
        );
        assert_eq!(
            parse_line(r#"{"event":"error","message":"could not reach the server"}"#),
            Some(LoginEvent::Failed("could not reach the server".into()))
        );
        assert_eq!(parse_line("not json"), None);
        assert_eq!(parse_line(r#"{"event":"later"}"#), None);
    }

    #[test]
    fn reads_server_url_from_agent_toml() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = AgentDirs {
            config: dir.path().into(),
            data: dir.path().into(),
        };
        assert_eq!(dirs.configured_server(), None);
        std::fs::write(
            dir.path().join("agent.toml"),
            "server_url = \"https://h:8443\"\n",
        )
        .unwrap();
        assert_eq!(dirs.configured_server().as_deref(), Some("https://h:8443"));
    }
}
