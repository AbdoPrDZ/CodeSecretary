//! End-to-end lifecycle tests against the real `secretary-agent` binary.

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_secretary-agent");

fn agent(home: &Path) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.env("SECRETARY_HOME", home)
        .env_remove("SECRETARY_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn logs(home: &Path) -> String {
    let Ok(entries) = std::fs::read_dir(home.join("logs")) else {
        return String::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("agent"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .collect()
}

fn wait_for_log(home: &Path, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if logs(home).contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn finish(child: Child) -> Output {
    child.wait_with_output().expect("agent process")
}

#[test]
fn starts_and_stops_cleanly() {
    let home = tempfile::tempdir().unwrap();
    let out = agent(home.path())
        .args(["run", "--run-for", "1"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for dir in ["config", "data", "logs"] {
        assert!(home.path().join(dir).is_dir(), "{dir} dir created");
    }
    let log = logs(home.path());
    assert!(log.contains("agent started"), "log: {log}");
    assert!(log.contains("agent stopped"), "log: {log}");
}

#[test]
fn second_instance_is_rejected_and_first_keeps_running() {
    let home = tempfile::tempdir().unwrap();
    let first = agent(home.path())
        .args(["run", "--run-for", "4"])
        .spawn()
        .unwrap();
    assert!(
        wait_for_log(home.path(), "agent started", Duration::from_secs(10)),
        "first instance did not start"
    );

    let second = agent(home.path())
        .args(["run", "--run-for", "1"])
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(3), "already-running exit code");
    assert!(String::from_utf8_lossy(&second.stderr).contains("already running"));

    let first = finish(first);
    assert!(first.status.success(), "first instance exits cleanly");

    // Lock is released after exit: a new instance can start.
    let third = agent(home.path())
        .args(["run", "--run-for", "1"])
        .output()
        .unwrap();
    assert!(third.status.success());
}

#[test]
fn invalid_config_exits_with_config_code() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("config")).unwrap();
    std::fs::write(
        home.path().join("config").join("agent.toml"),
        "server_url = \"ftp://nope\"\n",
    )
    .unwrap();

    let out = agent(home.path())
        .args(["run", "--run-for", "1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("server_url"));
}

#[test]
fn paths_command_prints_directories() {
    let home = tempfile::tempdir().unwrap();
    let out = agent(home.path()).arg("paths").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("config:") && stdout.contains("data:") && stdout.contains("logs:"));
}

#[cfg(unix)]
#[test]
fn sigterm_stops_gracefully() {
    let home = tempfile::tempdir().unwrap();
    let child = agent(home.path()).arg("run").spawn().unwrap();
    assert!(wait_for_log(
        home.path(),
        "agent started",
        Duration::from_secs(10)
    ));

    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());

    let out = finish(child);
    assert!(out.status.success(), "exit after SIGTERM: {:?}", out.status);
    let log = logs(home.path());
    assert!(log.contains("reason=\"SIGTERM\""), "log: {log}");
    assert!(log.contains("agent stopped"), "log: {log}");
}
