//! `codesecretary`: the program users start.
//!
//! It starts the agent daemon (`secretary-agent run`) as a separate background
//! process when it is not already running, then shows a small floating desktop
//! widget and a tray icon. The widget only reads `<data>/status.json`, which
//! the daemon rewrites every few seconds; closing the widget leaves the daemon
//! running. Sign-in runs `secretary-agent login --json` and shows the code.

// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod settings;
mod tray;
mod view;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::Utc;
use eframe::egui::{
    self, Color32, CornerRadius, Margin, RichText, Stroke, ViewportCommand, WindowLevel,
};
use secretary_shared::local::AgentStatus;

use crate::agent::{AgentDirs, LoginEvent, LoginProcess};
use crate::settings::WidgetSettings;
use crate::tray::{Tray, TrayAction};
use crate::view::{Level, Snapshot, ago, summary};

const REFRESH: Duration = Duration::from_secs(2);
const WIDTH: f32 = 300.0;

fn main() -> eframe::Result {
    let dirs = AgentDirs::resolve();
    let exe = agent::agent_exe();
    let settings = WidgetSettings::load(&dirs.data);

    // One program for the user: bring the daemon up if it is not running.
    if !matches!(read_status(&dirs), Snapshot::Status(s) if s.is_live(Utc::now())) {
        let _ = agent::start_daemon(&exe);
    }

    let viewport = egui::ViewportBuilder::default()
        .with_title("CodeSecretary")
        .with_inner_size([WIDTH, 190.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false)
        .with_visible(settings.visible)
        .with_window_level(if settings.on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        });
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "CodeSecretary",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let tray = Tray::create(move || ctx.request_repaint());
            Ok(Box::new(Widget::new(dirs, exe, settings, tray)))
        }),
    )
}

fn read_status(dirs: &AgentDirs) -> Snapshot {
    match std::fs::read(dirs.status_file()) {
        Ok(bytes) => match serde_json::from_slice::<AgentStatus>(&bytes) {
            Ok(status) => Snapshot::Status(Box::new(status)),
            Err(_) => Snapshot::Unreadable,
        },
        Err(_) => Snapshot::Missing,
    }
}

enum SignIn {
    Idle,
    /// Asking for the server URL.
    Form {
        server: String,
    },
    Waiting {
        process: LoginProcess,
        code: Option<(String, String)>,
    },
    Done(String),
    Failed(String),
}

struct Widget {
    dirs: AgentDirs,
    exe: PathBuf,
    settings: WidgetSettings,
    tray: Option<Tray>,
    snapshot: Snapshot,
    read_at: Instant,
    sign_in: SignIn,
    height: f32,
    /// Moved to the top-right corner of the screen yet (done once per launch).
    placed: bool,
}

impl Widget {
    fn new(dirs: AgentDirs, exe: PathBuf, settings: WidgetSettings, tray: Option<Tray>) -> Self {
        let snapshot = read_status(&dirs);
        Self {
            dirs,
            exe,
            settings,
            tray,
            snapshot,
            read_at: Instant::now(),
            sign_in: SignIn::Idle,
            height: 190.0,
            placed: false,
        }
    }

    fn status(&self) -> Option<&AgentStatus> {
        match &self.snapshot {
            Snapshot::Status(s) => Some(s),
            _ => None,
        }
    }

    fn server_guess(&self) -> String {
        self.status()
            .and_then(|s| s.account.as_ref().map(|a| a.server_url.clone()))
            .or_else(|| self.dirs.configured_server())
            .unwrap_or_else(|| "https://".into())
    }

    fn act(&mut self, ctx: &egui::Context, action: TrayAction) {
        match action {
            TrayAction::ToggleWidget => {
                self.settings.visible = !self.settings.visible;
                ctx.send_viewport_cmd(ViewportCommand::Visible(self.settings.visible));
                if self.settings.visible {
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
                self.settings.save(&self.dirs.data);
            }
            TrayAction::SignIn => {
                self.show(ctx);
                if matches!(
                    self.sign_in,
                    SignIn::Idle | SignIn::Done(_) | SignIn::Failed(_)
                ) {
                    self.sign_in = SignIn::Form {
                        server: self.server_guess(),
                    };
                }
            }
            TrayAction::SignOut => agent::logout(&self.exe),
            TrayAction::ToggleOnTop => {
                self.settings.on_top = !self.settings.on_top;
                ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if self.settings.on_top {
                    WindowLevel::AlwaysOnTop
                } else {
                    WindowLevel::Normal
                }));
                self.settings.save(&self.dirs.data);
            }
            TrayAction::OpenData => agent::open_folder(&self.dirs.data),
            TrayAction::StartAgent => {
                let _ = agent::start_daemon(&self.exe);
            }
            TrayAction::StopAgent => agent::stop_daemon(&self.exe),
            TrayAction::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        if !self.settings.visible {
            self.settings.visible = true;
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            self.settings.save(&self.dirs.data);
        }
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    fn poll_sign_in(&mut self) {
        if let SignIn::Waiting { process, code } = &mut self.sign_in {
            while let Ok(event) = process.events.try_recv() {
                match event {
                    LoginEvent::Code { url, user_code, .. } => *code = Some((url, user_code)),
                    LoginEvent::SignedIn { email } => {
                        self.sign_in = SignIn::Done(email.unwrap_or_else(|| "your account".into()));
                        return;
                    }
                    LoginEvent::Failed(message) => {
                        self.sign_in = SignIn::Failed(message);
                        return;
                    }
                }
            }
        }
    }

    /// On every launch the widget goes to the top-right corner of the screen:
    /// x = screen width − widget width − margin, y = margin. It can still be
    /// dragged elsewhere during a session.
    fn place_top_right(&mut self, ctx: &egui::Context) {
        if self.placed {
            return;
        }
        let (monitor, outer) = ctx.input(|i| (i.viewport().monitor_size, i.viewport().outer_rect));
        if let Some(monitor) = monitor {
            let width = outer.map_or(WIDTH, |r| r.width());
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(top_right(monitor, width)));
            self.placed = true;
        } else {
            ctx.request_repaint(); // the monitor size arrives with the next frame
        }
    }
}

impl eframe::App for Widget {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0] // transparent around the rounded card
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(tray) = &self.tray {
            let actions: Vec<_> = tray.actions.try_iter().collect();
            for action in actions {
                self.act(ctx, action);
            }
        }
        if self.read_at.elapsed() >= REFRESH {
            self.snapshot = read_status(&self.dirs);
            self.read_at = Instant::now();
        }
        self.poll_sign_in();
        self.place_top_right(ctx);

        let now = Utc::now();
        let (level, headline, detail) = summary(&self.snapshot, now);
        if let Some(tray) = &mut self.tray {
            tray.set_level(level);
        }
        let fast = matches!(self.sign_in, SignIn::Waiting { .. });
        ctx.request_repaint_after(if fast {
            Duration::from_millis(300)
        } else {
            REFRESH
        });

        let card = egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(17, 24, 39, 232))
            .stroke(Stroke::new(
                1.0_f32,
                Color32::from_rgba_unmultiplied(255, 255, 255, 40),
            ))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(14));

        let mut action = None;
        let response = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                // Registered first, so buttons drawn on top keep their clicks:
                // dragging any empty part of the card moves the widget.
                let background = ui.interact(
                    ui.max_rect(),
                    egui::Id::new("widget-drag"),
                    egui::Sense::drag(),
                );
                if background.drag_started() {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                ui.visuals_mut().override_text_color = Some(Color32::from_gray(230));
                card.show(ui, |ui| {
                    ui.set_width(WIDTH - 30.0);
                    // Header: drag handle + menu.
                    ui.horizontal(|ui| {
                        dot(ui, level);
                        ui.label(RichText::new(headline).strong().size(15.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("x")
                                .on_hover_text("Hide (the agent keeps running)")
                                .clicked()
                            {
                                action = Some(if self.tray.is_some() {
                                    TrayAction::ToggleWidget
                                } else {
                                    TrayAction::Quit
                                });
                            }
                            ui.menu_button("...", |ui| {
                                for (label, a) in menu_items(self.status()) {
                                    if ui.button(label).clicked() {
                                        action = Some(a);
                                        ui.close();
                                    }
                                }
                            });
                        });
                    });
                    if !detail.is_empty() {
                        ui.label(RichText::new(detail).small().color(Color32::from_gray(160)));
                    }
                    ui.add_space(6.0);
                    if let Some(status) = self.status().cloned()
                        && status.is_live(now)
                    {
                        facts(ui, &status, now);
                        ui.add_space(2.0);
                        ui.collapsing(RichText::new("Details").small(), |ui| {
                            for (section, items) in view::rows(&status, now) {
                                ui.label(RichText::new(section).small().strong());
                                egui::Grid::new(section)
                                    .num_columns(2)
                                    .spacing([12.0, 2.0])
                                    .show(ui, |ui| {
                                        for item in items {
                                            ui.label(
                                                RichText::new(item.label)
                                                    .small()
                                                    .color(Color32::from_gray(150)),
                                            );
                                            let text = RichText::new(item.value).small();
                                            match item.level {
                                                Some(l) => {
                                                    let [r, g, b] = tray::rgb(l);
                                                    ui.label(text.color(Color32::from_rgb(r, g, b)))
                                                }
                                                None => ui.label(text),
                                            };
                                            ui.end_row();
                                        }
                                    });
                            }
                        });
                    }
                    ui.add_space(4.0);
                    if let Some(a) = self.sign_in_ui(ui) {
                        action = Some(a);
                    }
                });
            });

        // Fit the window to the card.
        let height = response.response.rect.height().max(80.0);
        if (height - self.height).abs() > 1.0 {
            self.height = height;
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(WIDTH, height)));
        }
        if let Some(a) = action {
            self.act(ctx, a);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let SignIn::Waiting { .. } = self.sign_in
            && let SignIn::Waiting { process, .. } =
                std::mem::replace(&mut self.sign_in, SignIn::Idle)
        {
            process.cancel();
        }
    }
}

impl Widget {
    /// The sign-in area; returns an action for the caller to run.
    fn sign_in_ui(&mut self, ui: &mut egui::Ui) -> Option<TrayAction> {
        let signed_out = self.status().is_some_and(|s| s.account.is_none());
        let mut next = None;
        let mut action = None;
        match &mut self.sign_in {
            SignIn::Idle => {
                if signed_out && ui.button("Sign in…").clicked() {
                    action = Some(TrayAction::SignIn);
                }
            }
            SignIn::Form { server } => {
                ui.label(RichText::new("Server (warehouse URL)").small());
                ui.add(
                    egui::TextEdit::singleline(server)
                        .desired_width(f32::INFINITY)
                        .hint_text("https://host:8443"),
                );
                ui.horizontal(|ui| {
                    let valid = server.starts_with("http://") || server.starts_with("https://");
                    if ui
                        .add_enabled(valid && server.len() > 8, egui::Button::new("Sign in"))
                        .clicked()
                    {
                        next = Some(match LoginProcess::start(&self.exe, server.trim()) {
                            Ok(process) => SignIn::Waiting {
                                process,
                                code: None,
                            },
                            Err(err) => SignIn::Failed(format!("could not start the agent: {err}")),
                        });
                    }
                    if ui.button("Cancel").clicked() {
                        next = Some(SignIn::Idle);
                    }
                });
            }
            SignIn::Waiting { code, .. } => {
                match code {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Contacting the server…");
                        });
                    }
                    Some((url, user_code)) => {
                        ui.label(
                            RichText::new("Approve this device in the browser with the code")
                                .small(),
                        );
                        ui.label(
                            RichText::new(user_code.as_str())
                                .monospace()
                                .size(22.0)
                                .strong(),
                        );
                        if ui.button("Open browser").clicked() {
                            let _ = webbrowser::open(url);
                        }
                    }
                }
                if ui.small_button("Cancel").clicked() {
                    next = Some(SignIn::Idle);
                }
            }
            SignIn::Done(email) => {
                ui.colored_label(
                    Color32::from_rgb(16, 185, 129),
                    format!("Signed in as {email}"),
                );
                if ui.small_button("OK").clicked() {
                    next = Some(SignIn::Idle);
                }
            }
            SignIn::Failed(message) => {
                ui.colored_label(Color32::from_rgb(239, 68, 68), message.as_str());
                ui.horizontal(|ui| {
                    if ui.small_button("Try again").clicked() {
                        next = Some(SignIn::Form {
                            server: self.server_guess_static(),
                        });
                    }
                    if ui.small_button("Close").clicked() {
                        next = Some(SignIn::Idle);
                    }
                });
            }
        }
        // Leaving the waiting state by the user's choice cancels the login.
        if let Some(next) = next
            && let SignIn::Waiting { process, .. } = std::mem::replace(&mut self.sign_in, next)
        {
            process.cancel();
        }
        action
    }

    fn server_guess_static(&self) -> String {
        self.dirs
            .configured_server()
            .unwrap_or_else(|| "https://".into())
    }
}

/// Distance from the screen edges, in points.
const MARGIN: f32 = 16.0;

fn top_right(monitor: egui::Vec2, width: f32) -> egui::Pos2 {
    egui::pos2((monitor.x - width - MARGIN).max(0.0), MARGIN)
}

fn dot(ui: &mut egui::Ui, level: Level) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    let [r, g, b] = tray::rgb(level);
    ui.painter()
        .circle_filled(rect.center(), 6.0, Color32::from_rgb(r, g, b));
}

fn facts(ui: &mut egui::Ui, s: &AgentStatus, now: chrono::DateTime<Utc>) {
    let muted = Color32::from_gray(150);
    let row = |ui: &mut egui::Ui, label: &str, value: String| {
        ui.label(RichText::new(label).small().color(muted));
        ui.label(RichText::new(value).small());
        ui.end_row();
    };
    egui::Grid::new("facts")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            if let Some(a) = &s.account {
                row(
                    ui,
                    "Account",
                    a.email.clone().unwrap_or_else(|| "signed in".into()),
                );
            }
            row(ui, "Waiting", format!("{} event(s)", s.upload.queued));
            row(
                ui,
                "Last upload",
                s.upload.last_upload_at.map_or("none yet".into(), |t| {
                    format!("{} · {} events", ago(t, now), s.upload.last_upload_events)
                }),
            );
            row(
                ui,
                "You",
                match (s.presence.state.as_deref(), s.presence.since) {
                    (Some(state), Some(since)) => format!(
                        "{state} since {}",
                        since.with_timezone(&chrono::Local).format("%H:%M")
                    ),
                    (Some(state), None) => state.to_owned(),
                    _ => "—".into(),
                },
            );
            row(
                ui,
                "Rules",
                format!(
                    "{}{} exclusions · {} pause windows",
                    if s.config.paused { "paused · " } else { "" },
                    s.config.path_exclusions + s.config.app_exclusions + s.config.domain_exclusions,
                    s.config.pause_windows
                ),
            );
        });
}

fn menu_items(status: Option<&AgentStatus>) -> Vec<(&'static str, TrayAction)> {
    let now = Utc::now();
    let live = status.is_some_and(|s| s.is_live(now));
    let signed_in = status.is_some_and(|s| s.account.is_some());
    let mut items = vec![];
    if signed_in {
        items.push(("Sign out", TrayAction::SignOut));
    } else {
        items.push(("Sign in…", TrayAction::SignIn));
    }
    items.push(("Keep on top", TrayAction::ToggleOnTop));
    items.push(("Open data folder", TrayAction::OpenData));
    items.push(if live {
        ("Stop agent", TrayAction::StopAgent)
    } else {
        ("Start agent", TrayAction::StartAgent)
    });
    items.push(("Close widget", TrayAction::Quit));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_right_is_computed_from_the_screen_size() {
        assert_eq!(
            top_right(egui::vec2(1536.0, 864.0), 300.0),
            egui::pos2(1220.0, 16.0)
        );
        assert_eq!(
            top_right(egui::vec2(1920.0, 1080.0), 300.0),
            egui::pos2(1604.0, 16.0)
        );
        assert_eq!(
            top_right(egui::vec2(200.0, 600.0), 300.0).x,
            0.0,
            "never off the left edge"
        );
    }
}
