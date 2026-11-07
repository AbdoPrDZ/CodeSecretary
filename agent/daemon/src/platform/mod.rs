//! OS integration for presence (idle time) and focus (foreground app/title).
//!
//! One module per OS exposes the same `Native` type:
//!
//! | OS | Idle | Focus |
//! |---|---|---|
//! | Windows | `GetLastInputInfo` | `GetForegroundWindow` + `GetWindowTextW` + process image name |
//! | macOS | `CGEventSourceSecondsSinceLastEventType` | `CGWindowListCopyWindowInfo` (app); Accessibility API (title, needs permission) |
//! | Linux X11 | XScreenSaver `QueryInfo` | `_NET_ACTIVE_WINDOW`, `_NET_WM_NAME`, `WM_CLASS`, `_NET_WM_PID` |
//! | Linux Wayland | best-effort via XWayland only (see [`session_notes`]) | same |
//!
//! PRIVACY: window titles returned here are raw. They must go through
//! redaction (`shared`) before being buffered, and must never be logged.

use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux_x11;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod unsupported;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux_x11 as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
use unsupported as imp;
#[cfg(windows)]
use windows as imp;

/// The foreground window as reported by the OS. Fields are `None` when the
/// OS does not expose them or permission is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedWindow {
    /// Application / executable name (e.g. `Code.exe`, `Safari`, `firefox`).
    pub app: Option<String>,
    /// Raw window title. Sensitive — redact before storing, never log.
    pub title: Option<String>,
    pub pid: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // each OS uses a different subset of variants
pub enum PlatformError {
    #[error("not supported here: {0}")]
    Unsupported(String),
    #[error("{api} failed: {source}")]
    Os {
        api: &'static str,
        source: std::io::Error,
    },
    #[error("{0}")]
    Other(String),
}

impl PlatformError {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn last_os(api: &'static str) -> Self {
        Self::Os {
            api,
            source: std::io::Error::last_os_error(),
        }
    }
}

/// Connection to the OS presence/focus APIs.
pub struct Platform {
    native: imp::Native,
}

impl Platform {
    pub fn connect() -> Result<Self, PlatformError> {
        Ok(Self {
            native: imp::Native::connect()?,
        })
    }

    /// Time since the last keyboard/mouse input in the user's session.
    pub fn idle_time(&mut self) -> Result<Duration, PlatformError> {
        self.native.idle_time()
    }

    /// The current foreground window, or `None` if there is none
    /// (e.g. desktop focused, lock screen on some OSes).
    pub fn focused_window(&mut self) -> Result<Option<FocusedWindow>, PlatformError> {
        self.native.focused_window()
    }

    /// Whether window titles can be read (e.g. macOS Accessibility permission).
    pub fn titles_available(&self) -> bool {
        self.native.titles_available()
    }
}

/// Human-readable notes about the current session that affect accuracy
/// (e.g. Wayland). Empty when nothing notable.
pub fn session_notes() -> Vec<String> {
    imp::session_notes()
}
