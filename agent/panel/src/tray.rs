//! Tray icon: a colored dot (green online, amber attention, red offline, grey
//! stopped) and a menu. Menu clicks arrive as [`TrayAction`]s on a channel and
//! wake the widget.
//!
//! Windows and macOS: created on the main (event-loop) thread. Linux: GTK +
//! AppIndicator on a thread of its own running the GTK loop; the icon color
//! follows a shared level polled there.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use crate::view::Level;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ToggleWidget,
    SignIn,
    SignOut,
    ToggleOnTop,
    OpenData,
    StartAgent,
    StopAgent,
    Quit,
}

const ITEMS: &[(&str, &str, TrayAction)] = &[
    ("toggle", "Show / hide widget", TrayAction::ToggleWidget),
    ("signin", "Sign in…", TrayAction::SignIn),
    ("signout", "Sign out", TrayAction::SignOut),
    ("ontop", "Keep widget on top", TrayAction::ToggleOnTop),
    ("data", "Open data folder", TrayAction::OpenData),
    ("start", "Start agent", TrayAction::StartAgent),
    ("stop", "Stop agent", TrayAction::StopAgent),
    (
        "quit",
        "Close widget (agent keeps running)",
        TrayAction::Quit,
    ),
];

pub struct Tray {
    pub actions: Receiver<TrayAction>,
    level: Arc<AtomicU8>,
    #[cfg(not(target_os = "linux"))]
    icon: tray_icon::TrayIcon,
    #[cfg(not(target_os = "linux"))]
    shown: Level,
}

impl Tray {
    /// `wake` is called (from any thread) after every action, so the widget
    /// processes it even while hidden.
    pub fn create(wake: impl Fn() + Send + Sync + 'static) -> Option<Self> {
        let (tx, actions) = mpsc::channel();
        let wake = Arc::new(wake);
        {
            let tx = tx.clone();
            let wake = wake.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some(action) = action_for(&event.id) {
                    let _ = tx.send(action);
                    wake();
                }
            }));
        }
        let level = Arc::new(AtomicU8::new(level_code(Level::Off)));

        #[cfg(not(target_os = "linux"))]
        {
            let wake = wake.clone();
            tray_icon::TrayIconEvent::set_event_handler(Some(
                move |event: tray_icon::TrayIconEvent| {
                    if let tray_icon::TrayIconEvent::Click {
                        button: tray_icon::MouseButton::Left,
                        button_state: tray_icon::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let _ = tx.send(TrayAction::ToggleWidget);
                        wake();
                    }
                },
            ));
            let icon = build().ok()?;
            Some(Self {
                actions,
                level,
                icon,
                shown: Level::Off,
            })
        }

        #[cfg(target_os = "linux")]
        {
            drop(tx);
            let shared = level.clone();
            std::thread::spawn(move || linux_loop(shared));
            Some(Self { actions, level })
        }
    }

    pub fn set_level(&mut self, level: Level) {
        self.level.store(level_code(level), Ordering::Relaxed);
        #[cfg(not(target_os = "linux"))]
        if level != self.shown {
            self.shown = level;
            let _ = self.icon.set_icon(Some(icon(level)));
            let _ = self.icon.set_tooltip(Some(tooltip(level)));
        }
    }
}

fn action_for(id: &MenuId) -> Option<TrayAction> {
    ITEMS
        .iter()
        .find(|(key, _, _)| id.0 == *key)
        .map(|(_, _, a)| *a)
}

fn menu() -> Result<Menu, tray_icon::menu::Error> {
    let menu = Menu::new();
    for (i, (key, label, _)) in ITEMS.iter().enumerate() {
        if i == 4 || i == 5 || i == 7 {
            menu.append(&PredefinedMenuItem::separator())?;
        }
        menu.append(&MenuItem::with_id(*key, *label, true, None))?;
    }
    Ok(menu)
}

fn build() -> Result<tray_icon::TrayIcon, Box<dyn std::error::Error>> {
    Ok(TrayIconBuilder::new()
        .with_menu(Box::new(menu()?))
        .with_tooltip(tooltip(Level::Off))
        .with_icon(icon(Level::Off))
        .build()?)
}

#[cfg(target_os = "linux")]
fn linux_loop(level: Arc<AtomicU8>) {
    use std::time::Duration;

    if gtk::init().is_err() {
        return; // no display or GTK: the widget works without a tray
    }
    let Ok(tray) = build() else { return };
    let mut shown = u8::MAX;
    gtk::glib::timeout_add_local(Duration::from_secs(2), move || {
        let now = level.load(Ordering::Relaxed);
        if now != shown {
            shown = now;
            let level = level_from(now);
            let _ = tray.set_icon(Some(icon(level)));
            let _ = tray.set_tooltip(Some(tooltip(level)));
        }
        gtk::glib::ControlFlow::Continue
    });
    gtk::main();
}

fn level_code(level: Level) -> u8 {
    match level {
        Level::Good => 0,
        Level::Warn => 1,
        Level::Bad => 2,
        Level::Off => 3,
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn level_from(code: u8) -> Level {
    match code {
        0 => Level::Good,
        1 => Level::Warn,
        2 => Level::Bad,
        _ => Level::Off,
    }
}

fn tooltip(level: Level) -> &'static str {
    match level {
        Level::Good => "CodeSecretary — connected",
        Level::Warn => "CodeSecretary — needs attention",
        Level::Bad => "CodeSecretary — server unreachable",
        Level::Off => "CodeSecretary — agent not running",
    }
}

pub fn rgb(level: Level) -> [u8; 3] {
    match level {
        Level::Good => [16, 185, 129],
        Level::Warn => [245, 158, 11],
        Level::Bad => [239, 68, 68],
        Level::Off => [148, 163, 184],
    }
}

/// A 32×32 dot with a light ring, anti-aliased, in `level`'s color.
fn icon(level: Level) -> Icon {
    const N: usize = 32;
    let [r, g, b] = rgb(level);
    let mut rgba = vec![0u8; N * N * 4];
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let px = &mut rgba[(y * N + x) * 4..][..4];
            let outer = (15.5 - d).clamp(0.0, 1.0);
            if outer <= 0.0 {
                continue;
            }
            let inner = (12.5 - d).clamp(0.0, 1.0); // fill; outside it a white ring
            let mix = |fill: u8| (fill as f32 * inner + 255.0 * (1.0 - inner)) as u8;
            px.copy_from_slice(&[mix(r), mix(g), mix(b), (outer * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, N as u32, N as u32).expect("valid icon")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_actions() {
        assert_eq!(action_for(&MenuId::new("signin")), Some(TrayAction::SignIn));
        assert_eq!(action_for(&MenuId::new("quit")), Some(TrayAction::Quit));
        assert_eq!(action_for(&MenuId::new("nope")), None);
        for level in [Level::Good, Level::Warn, Level::Bad, Level::Off] {
            assert_eq!(level_from(level_code(level)), level);
            let _ = icon(level);
        }
    }
}
