//! Per-user directories for config, data (buffer, lock) and logs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Overrides all agent directories with `<value>/{config,data,logs}`.
/// Used by tests and portable/dev setups.
pub const HOME_ENV: &str = "SECRETARY_HOME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl AgentPaths {
    /// Resolves the directories from `SECRETARY_HOME` or the OS conventions:
    ///
    /// | OS | config | data |
    /// |---|---|---|
    /// | Linux | `~/.config/secretary` | `~/.local/share/secretary` |
    /// | macOS | `~/Library/Application Support/secretary` | same |
    /// | Windows | `%APPDATA%\secretary\config` | `%LOCALAPPDATA%\secretary\data` |
    ///
    /// Logs live in `<data>/logs`.
    pub fn resolve() -> Result<Self> {
        if let Some(home) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::under(Path::new(&home)));
        }
        let dirs = directories::ProjectDirs::from("", "", "secretary")
            .context("could not determine the current user's home directory")?;
        let data_dir = dirs.data_local_dir().to_path_buf();
        Ok(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            log_dir: data_dir.join("logs"),
            data_dir,
        })
    }

    pub fn under(root: &Path) -> Self {
        Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            log_dir: root.join("logs"),
        }
    }

    /// Local bootstrap config (server URL, log level). Remote config is separate (AG-7).
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("agent.toml")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.data_dir.join("agent.lock")
    }

    /// Sign-in state (no secrets): installation id, Keycloak endpoints, device id.
    pub fn state_file(&self) -> PathBuf {
        self.data_dir.join("state.json")
    }

    /// Written by `secretary-agent stop` (and the widget); a running agent
    /// that sees it removes it and shuts down.
    pub fn stop_file(&self) -> PathBuf {
        self.data_dir.join("stop.request")
    }

    /// Status for the panel (`secretary_shared::local`), rewritten every few seconds.
    pub fn status_file(&self) -> PathBuf {
        self.data_dir.join(secretary_shared::local::STATUS_FILE)
    }

    /// Last `AgentConfig` fetched from the warehouse, with its `ETag` (AG-7).
    pub fn agent_config_file(&self) -> PathBuf {
        self.data_dir.join("agent_config.json")
    }

    /// Local event buffer (SQLite), holding redacted events until acknowledged.
    pub fn buffer_file(&self) -> PathBuf {
        self.data_dir.join("buffer.sqlite3")
    }

    /// Creates all directories, readable by the current user only.
    pub fn ensure(&self) -> Result<()> {
        for dir in [&self.config_dir, &self.data_dir, &self.log_dir] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("could not create directory {}", dir.display()))?;
            restrict_to_user(dir)
                .with_context(|| format!("could not set permissions on {}", dir.display()))?;
        }
        Ok(())
    }
}

#[cfg(unix)]
fn restrict_to_user(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_to_user(_dir: &Path) -> std::io::Result<()> {
    // Windows: directories under %APPDATA% / %LOCALAPPDATA% inherit the
    // profile's per-user ACLs, which already exclude other users.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_root_uses_fixed_layout() {
        let root = Path::new("/tmp/secretary-test");
        let paths = AgentPaths::under(root);
        assert_eq!(paths.config_dir, root.join("config"));
        assert_eq!(paths.data_dir, root.join("data"));
        assert_eq!(paths.log_dir, root.join("logs"));
        assert_eq!(paths.config_file(), root.join("config").join("agent.toml"));
        assert_eq!(paths.lock_file(), root.join("data").join("agent.lock"));
    }

    #[test]
    fn ensure_creates_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AgentPaths::under(tmp.path());
        paths.ensure().unwrap();
        assert!(paths.config_dir.is_dir());
        assert!(paths.data_dir.is_dir());
        assert!(paths.log_dir.is_dir());
        // Idempotent.
        paths.ensure().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ensure_restricts_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let paths = AgentPaths::under(tmp.path());
        paths.ensure().unwrap();
        let mode = std::fs::metadata(&paths.data_dir)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
