//! Linux (X11): XScreenSaver for idle; EWMH properties for focus.
//!
//! Wayland: there is no portable idle/focus API. When only XWayland is
//! available, idle and focus reflect X11 clients only, so this is
//! best-effort — see [`session_notes`]. Native Wayland support
//! (`ext-idle-notify-v1`, GNOME Mutter IdleMonitor over D-Bus) is AG-8.

use std::time::Duration;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::screensaver::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use super::{FocusedWindow, PlatformError};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_ACTIVE_WINDOW,
        _NET_WM_NAME,
        _NET_WM_PID,
        UTF8_STRING,
    }
}

pub struct Native {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    has_screensaver: bool,
}

fn err(context: &str, e: impl std::fmt::Display) -> PlatformError {
    PlatformError::Other(format!("{context}: {e}"))
}

impl Native {
    pub fn connect() -> Result<Self, PlatformError> {
        if std::env::var_os("DISPLAY").is_none() {
            return Err(PlatformError::Unsupported(
                "no X11 display (DISPLAY unset). Pure Wayland sessions are not supported yet (AG-8)."
                    .to_owned(),
            ));
        }
        let (conn, screen) = x11rb::connect(None).map_err(|e| err("X11 connect", e))?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn)
            .map_err(|e| err("X11 intern atoms", e))?
            .reply()
            .map_err(|e| err("X11 intern atoms", e))?;
        let has_screensaver = conn
            .extension_information(screensaver::X11_EXTENSION_NAME)
            .map_err(|e| err("X11 query extension", e))?
            .is_some();
        Ok(Self {
            conn,
            root,
            atoms,
            has_screensaver,
        })
    }

    pub fn idle_time(&mut self) -> Result<Duration, PlatformError> {
        if !self.has_screensaver {
            return Err(PlatformError::Unsupported(
                "X server lacks the MIT-SCREEN-SAVER extension".to_owned(),
            ));
        }
        let info = self
            .conn
            .screensaver_query_info(self.root)
            .map_err(|e| err("XScreenSaverQueryInfo", e))?
            .reply()
            .map_err(|e| err("XScreenSaverQueryInfo", e))?;
        Ok(Duration::from_millis(u64::from(info.ms_since_user_input)))
    }

    pub fn focused_window(&mut self) -> Result<Option<FocusedWindow>, PlatformError> {
        let active = self
            .conn
            .get_property(
                false,
                self.root,
                self.atoms._NET_ACTIVE_WINDOW,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .map_err(|e| err("_NET_ACTIVE_WINDOW", e))?
            .reply()
            .map_err(|e| err("_NET_ACTIVE_WINDOW", e))?;
        let Some(window) = active.value32().and_then(|mut v| v.next()) else {
            return Ok(None);
        };
        if window == 0 {
            return Ok(None);
        }

        let title = self
            .string_property(window, self.atoms._NET_WM_NAME, self.atoms.UTF8_STRING)
            .or_else(|| {
                self.string_property(window, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into())
            });
        let pid = self.pid(window);
        let app = self.wm_class(window).or_else(|| pid.and_then(proc_comm));
        Ok(Some(FocusedWindow { app, title, pid }))
    }

    pub fn titles_available(&self) -> bool {
        true
    }

    fn string_property(&self, window: Window, property: u32, kind: u32) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, window, property, kind, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        let text = String::from_utf8_lossy(&reply.value).into_owned();
        (!text.is_empty()).then_some(text)
    }

    fn pid(&self, window: Window) -> Option<u32> {
        let reply = self
            .conn
            .get_property(
                false,
                window,
                self.atoms._NET_WM_PID,
                AtomEnum::CARDINAL,
                0,
                1,
            )
            .ok()?
            .reply()
            .ok()?;
        reply.value32().and_then(|mut v| v.next())
    }

    /// `WM_CLASS` is `instance\0class\0`; the class (e.g. `firefox`, `Code`) names the app.
    fn wm_class(&self, window: Window) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        let mut parts = reply
            .value
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned());
        let instance = parts.next();
        parts.next().or(instance)
    }
}

fn proc_comm(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let comm = comm.trim();
    (!comm.is_empty()).then(|| comm.to_owned())
}

pub fn session_notes() -> Vec<String> {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let mut notes = vec![format!(
        "session type: {}, desktop: {}",
        if session.is_empty() {
            "unknown"
        } else {
            &session
        },
        if desktop.is_empty() {
            "unknown"
        } else {
            &desktop
        },
    )];
    if session.eq_ignore_ascii_case("wayland") || std::env::var_os("WAYLAND_DISPLAY").is_some() {
        notes.push(
            "Wayland session: data comes from XWayland and only reflects X11 apps — idle time and \
             focus may be wrong for native Wayland apps (best-effort; native support is AG-8)."
                .to_owned(),
        );
    }
    notes
}
