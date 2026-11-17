//! Layout-driven UI. All dimensions are logical points; eframe owns the
//! monitor scale, font atlas resolution, hit testing and physical surface.
use crate::controller::{CheckState, Cmd, Controller, Status, UpdateState};
use crate::draw::{self, Icon};
use crate::theme;
use egui::{vec2, Align2, Color32, FontFamily, FontId, Pos2, RichText, Sense, Stroke};
use std::time::Duration;

pub struct App {
    ctrl: Controller,
    status: Status,
    dismissed_error: Option<String>,
    #[cfg(not(windows))]
    shot_requested: bool,
    #[cfg(windows)]
    was_visible: bool,
}

impl App {
    pub fn new(ctrl: Controller) -> Self {
        Self {
            ctrl,
            status: Status::default(),
            dismissed_error: None,
            #[cfg(not(windows))]
            shot_requested: false,
            #[cfg(windows)]
            was_visible: false,
        }
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        let pal = theme::palette(ui.ctx());
        ui.painter().rect_filled(ui.max_rect(), 0, pal.bg);
        ui.spacing_mut().item_spacing = vec2(12.0, 10.0);
        // Reserve scrollbar space so it cannot intercept clicks on controls
        // underneath, especially on tiny high-DPI viewports.
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        // Very high scaling on a small display can leave fewer than 300
        // logical points. Keep controls usable by scrolling horizontally
        // only in that case, instead of forcing a window wider than the screen.
        let narrow = ui.available_width() < 300.0;
        let mut scroll = egui::ScrollArea::new([narrow, true]);
        if !narrow {
            // Disabling an axis alone retains its previous offset for one
            // frame. Clear it immediately when returning to a normal width.
            scroll = scroll.horizontal_scroll_offset(0.0);
        }
        scroll
            .id_salt("dashboard")
            .min_scrolled_width(0.0)
            .min_scrolled_height(0.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width().max(300.0));
                egui::Frame::new().inner_margin(16).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let (logo, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                        draw::brand_mark(
                            ui.painter(),
                            logo,
                            pal.accent_a,
                            Color32::from_rgb(34, 211, 238),
                            pal.text,
                        );
                        ui.label(
                            RichText::new("AION STABILIZER")
                                .size(12.5)
                                .family(FontFamily::Name("sb".into()))
                                .extra_letter_spacing(1.5)
                                .color(pal.text),
                        );
                        ui.label(
                            RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                                .size(11.0)
                                .color(pal.dim),
                        );
                    });
                    ui.add_space(6.0);
                    // Wider windows use the extra space, while narrow windows
                    // stack the cards. Text wraps at the actual available width.
                    if ui.available_width() >= 760.0 {
                        ui.columns(2, |cols| {
                            self.hero(&mut cols[0]);
                            self.settings(&mut cols[1]);
                        });
                    } else {
                        self.hero(ui);
                        ui.add_space(2.0);
                        self.settings(ui);
                    }
                    if let Some(error) = self.status.error.clone() {
                        if self.dismissed_error.as_ref() != Some(&error) {
                            egui::Frame::new()
                                .fill(pal.danger_bg)
                                .inner_margin(12)
                                .corner_radius(8)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.colored_label(pal.danger, &error);
                                    ui.label(
                                        "Engine details are saved in engine.log beside the app.",
                                    );
                                    if ui.button("Dismiss").clicked() {
                                        self.dismissed_error = Some(error);
                                    }
                                });
                        }
                    }
                    if let Some(update) = self.status.update.clone() {
                        Self::hairline(ui);
                        match update.state {
                            UpdateState::Available => {
                                ui.label(format!("Version {} is available.", update.version));
                                if ui.button("Install update").clicked() {
                                    self.ctrl.send(Cmd::ApplyUpdate);
                                }
                            }
                            UpdateState::Downloading(p) => {
                                ui.label(format!("Downloading update… {p}%"));
                            }
                            UpdateState::Verifying => {
                                ui.label("Verifying update…");
                            }
                            UpdateState::Installing => {
                                ui.label("Installing update. The app will restart.");
                            }
                            UpdateState::Failed(error) => {
                                ui.colored_label(pal.danger, format!("Update failed: {error}"));
                                if ui.button("Retry update").clicked() {
                                    self.ctrl.send(Cmd::ApplyUpdate);
                                }
                            }
                        }
                    }
                    Self::hairline(ui);
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 7.0;
                        let (icon, _) = ui.allocate_exact_size(vec2(13.0, 13.0), Sense::hover());
                        draw::icon(ui.painter(), Icon::Tray, icon, pal.dim);
                        ui.label(
                            RichText::new("Closing or minimizing keeps the app in the tray.")
                                .size(11.0)
                                .color(pal.dim),
                        );
                    });
                });
                // egui tracks keyboard focus independently of scrolling.
                // Reveal a newly focused control, including controls reached
                // by Tab while they were below or beside the viewport.
                if let Some(id) = ui.ctx().memory(|m| m.focused()) {
                    if let Some(response) = ui.ctx().read_response(id) {
                        if response.gained_focus() {
                            response.scroll_to_me(None);
                        }
                    }
                }
            });
    }

    fn card(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
        let pal = theme::palette(ui.ctx());
        egui::Frame::new()
            .fill(pal.card)
            .stroke(egui::Stroke::new(1.0, pal.card_border))
            .corner_radius(14)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                contents(ui);
            });
    }

    /// Hairline divider matching the card borders' 1pt rows.
    fn hairline(ui: &mut egui::Ui) {
        ui.add(egui::Separator::default().spacing(1.0));
    }

    /// Small tracked section label shared by both cards.
    fn eyebrow(text: &str, pal: theme::Palette) -> RichText {
        RichText::new(text)
            .size(10.5)
            .family(FontFamily::Name("sb".into()))
            .extra_letter_spacing(1.6)
            .color(pal.dim)
    }

    /// Status pill under the power button; optional leading icon.
    fn chip(ui: &mut egui::Ui, icon: Option<Icon>, text: &str, color: Color32) {
        egui::Frame::new()
            .fill(color.gamma_multiply(0.10))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if let Some(kind) = icon {
                        let (rect, _) = ui.allocate_exact_size(vec2(11.0, 11.0), Sense::hover());
                        draw::icon(ui.painter(), kind, rect, color);
                    }
                    ui.label(RichText::new(text).size(11.0).color(color));
                });
            });
    }

    fn hero(&mut self, ui: &mut egui::Ui) {
        Self::card(ui, |ui| {
            let running = self.status.running;
            let busy = self.status.update.as_ref().is_some_and(|u| {
                matches!(
                    u.state,
                    UpdateState::Downloading(_) | UpdateState::Verifying | UpdateState::Installing
                )
            });
            let pal = theme::palette(ui.ctx());
            let on = Color32::from_rgb(16, 185, 129);
            let end = draw::success(ui.ctx());
            let label = if running {
                "Stop engine"
            } else {
                "Start engine"
            };
            let response = ui
                .add_enabled_ui(!busy, |ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 161.0), Sense::click());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label)
                    });
                    let center = Pos2::new(rect.center().x, rect.top() + 64.0);
                    let painter = ui.painter();
                    let feather = draw::feather(ui.ctx());
                    let radius = if response.is_pointer_button_down_on() {
                        40.0
                    } else {
                        42.0
                    };
                    // Static glow, painted in native-resolution vectors. No idle
                    // pulse or CPU animation loop while the engine is running.
                    if running {
                        let boost = if response.hovered() { 1.35 } else { 1.0 };
                        for i in 0..3 {
                            painter.circle_stroke(
                                center,
                                49.0 + i as f32 * 5.0,
                                Stroke::new(
                                    7.0,
                                    on.gamma_multiply((0.10 - i as f32 * 0.025) * boost),
                                ),
                            );
                        }
                    } else if response.hovered() {
                        // Restrained hover highlight: a soft accent halo.
                        for (r, alpha) in [
                            (radius + 9.0, 0.10),
                            (radius + 12.5, 0.05),
                            (radius + 16.0, 0.025),
                        ] {
                            painter.circle_stroke(
                                center,
                                r,
                                Stroke::new(4.0, pal.accent_a.gamma_multiply(alpha)),
                            );
                        }
                    }
                    // Disc first: the ring's inner coverage fringe blends into
                    // it, so the junction is smooth instead of a hard seam.
                    painter.circle_filled(
                        center,
                        radius + feather * 0.5,
                        if running {
                            draw::lerp(on, pal.card, 0.88)
                        } else {
                            pal.card
                        },
                    );
                    draw::gradient_ring(
                        painter,
                        center,
                        radius,
                        radius + 6.0,
                        if running { on } else { pal.accent_a },
                        if running {
                            end
                        } else {
                            Color32::from_rgb(34, 211, 238)
                        },
                    );
                    draw::icon(
                        painter,
                        Icon::Power,
                        egui::Rect::from_center_size(center, vec2(44.0, 44.0)),
                        if running || response.hovered() {
                            pal.text
                        } else {
                            pal.dim
                        },
                    );
                    // Small tracked section label, painted like the status and
                    // action labels below the ring.
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        "ENGINE",
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(10.5, FontFamily::Name("sb".into())),
                            color: pal.dim,
                            extra_letter_spacing: 1.6,
                            ..Default::default()
                        },
                    );
                    let galley = painter.layout_job(job);
                    painter.galley(
                        Pos2::new(
                            rect.center().x - galley.size().x / 2.0,
                            rect.top() + 7.0 - galley.size().y / 2.0,
                        ),
                        galley,
                        pal.dim,
                    );
                    painter.text(
                        Pos2::new(rect.center().x, rect.top() + 125.0),
                        Align2::CENTER_CENTER,
                        if running { "RUNNING" } else { "STOPPED" },
                        FontId::new(19.0, FontFamily::Name("sb".into())),
                        if running { end } else { pal.text },
                    );
                    painter.text(
                        Pos2::new(rect.center().x, rect.top() + 146.0),
                        Align2::CENTER_CENTER,
                        label,
                        FontId::proportional(12.5),
                        pal.dim,
                    );
                    if response.has_focus() {
                        painter.circle_stroke(
                            center,
                            radius + 10.5,
                            Stroke::new(1.8, pal.accent_a),
                        );
                    }
                    response.on_hover_cursor(egui::CursorIcon::PointingHand)
                })
                .inner;
            if response.clicked() {
                self.ctrl.send(if running {
                    Cmd::Stop { manual: true }
                } else {
                    Cmd::Start { auto: false }
                });
            }
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                let game = if !self.status.watch {
                    "Detection off"
                } else if self.status.game_present {
                    "Aion detected"
                } else {
                    "Waiting for Aion"
                };
                Self::chip(
                    ui,
                    Some(Icon::Game),
                    game,
                    if self.status.game_present {
                        end
                    } else {
                        pal.dim
                    },
                );
                Self::chip(
                    ui,
                    None,
                    if self.status.auto_started && running {
                        "AUTO"
                    } else {
                        "MANUAL"
                    },
                    pal.accent_a,
                );
            });
            ui.add_space(2.0);
            ui.collapsing(RichText::new("Connection help").size(12.0).color(pal.dim), |ui| {
                ui.label("Start the engine before opening Aion, or enable Start engine on app open. Game detection starts only after the game opens. The default follows AXV’s compatibility rules with targeted game TCP. Broad game UDP injection is disabled. If login fails, stop the engine and retry. A running engine does not confirm game connectivity.");
            });
        });
    }

    fn setting(ui: &mut egui::Ui, icon: Icon, title: &str, subtitle: &str, value: &mut bool) -> bool {
        let pal = theme::palette(ui.ctx());
        let row = ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 0.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let (icon_rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                ui.add_space(10.0);
                let text_width = (ui.available_width() - 54.0).max(80.0);
                ui.allocate_ui_with_layout(
                    vec2(text_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(text_width);
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(
                            RichText::new(title)
                                .size(13.0)
                                .family(FontFamily::Name("md".into()))
                                .color(pal.text),
                        );
                        if !subtitle.is_empty() {
                            ui.label(RichText::new(subtitle).size(11.0).color(pal.dim));
                        }
                    },
                );
                let (switch_rect, _) = ui.allocate_exact_size(vec2(44.0, 32.0), Sense::hover());
                (icon_rect, switch_rect)
            },
        );
        let (icon_rect, switch_rect) = row.inner;
        let mut response = ui.interact(
            row.response.rect,
            ui.make_persistent_id(title),
            Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *value, title)
        });
        if response.clicked() {
            *value = !*value;
            response.mark_changed();
        }
        // Paint after interaction so the visuals can answer hover and focus.
        let painter = ui.painter();
        draw::icon(
            painter,
            icon,
            icon_rect,
            if response.hovered() { pal.text } else { pal.dim },
        );
        let track = egui::Rect::from_center_size(switch_rect.center(), vec2(36.0, 20.0));
        let track_col = if *value { pal.accent_a } else { pal.off_a };
        painter.rect_filled(
            track,
            10,
            if response.hovered() {
                draw::lerp(track_col, pal.text, 0.08)
            } else {
                track_col
            },
        );
        painter.circle_filled(
            Pos2::new(
                if *value {
                    track.right() - 10.0
                } else {
                    track.left() + 10.0
                },
                track.center().y,
            ),
            7.0,
            if *value { Color32::WHITE } else { pal.dim },
        );
        if response.has_focus() {
            painter.rect_stroke(
                response.rect.expand(2.0),
                8,
                Stroke::new(1.5, pal.accent_a),
                egui::StrokeKind::Inside,
            );
        }
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .changed()
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        Self::card(ui, |ui| {
            let pal = theme::palette(ui.ctx());
            ui.label(Self::eyebrow("SETTINGS", pal));
            ui.add_space(2.0);
            let mut watch = self.status.watch;
            if Self::setting(
                ui,
                Icon::Game,
                "Auto-start with Aion",
                "Start and stop with the game",
                &mut watch,
            ) {
                self.ctrl.send(Cmd::SetWatch(watch));
            }
            Self::hairline(ui);
            let mut startup = self.status.startup_engine;
            if Self::setting(
                ui,
                Icon::Power,
                "Start engine on app open",
                "Turn on when this app opens",
                &mut startup,
            ) {
                self.ctrl.send(Cmd::SetStartupEngine(startup));
            }
            Self::hairline(ui);
            let mut autostart = self.status.autostart;
            if Self::setting(
                ui,
                Icon::Window,
                "Launch with Windows",
                "Start in the tray on login",
                &mut autostart,
            ) {
                self.ctrl.send(Cmd::SetAutostart(autostart));
            }
            Self::hairline(ui);
            ui.add_space(2.0);
            let checking = matches!(self.status.check, CheckState::Checking);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                let (kind, tint) = match self.status.check {
                    CheckState::Checking => (Icon::Update, pal.dim),
                    CheckState::UpToDate => (Icon::Check, draw::success(ui.ctx())),
                    CheckState::Failed => (Icon::Update, pal.danger),
                    CheckState::Idle => (Icon::Update, pal.dim),
                };
                let (rect, _) = ui.allocate_exact_size(vec2(13.0, 13.0), Sense::hover());
                draw::icon(ui.painter(), kind, rect, tint);
                ui.label(
                    RichText::new(match self.status.check {
                        CheckState::Checking => "Checking for updates…",
                        CheckState::UpToDate => "Latest version installed.",
                        CheckState::Failed => "Update check failed. Try again.",
                        CheckState::Idle => "Updates are checked on startup.",
                    })
                    .size(11.0)
                    .color(pal.dim),
                );
            });
            let button = egui::Button::new(
                RichText::new("Check for updates")
                    .size(12.0)
                    .color(pal.accent_a),
            )
            .fill(pal.accent_a.gamma_multiply(0.12))
            .stroke(Stroke::new(1.0, pal.accent_a.gamma_multiply(0.25)))
            .corner_radius(12);
            if ui.add_enabled(!checking, button).clicked() {
                self.ctrl.send(Cmd::CheckUpdate);
            }
        });
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.status = self.ctrl.status();
        #[cfg(not(windows))]
        if std::env::var_os("AION_PREVIEW_RUNNING").is_some() {
            self.status.running = true;
            self.status.game_present = true;
            self.status.auto_started = true;
        }
        #[cfg(windows)]
        {
            let visible = crate::sys::main_window_visible();
            if visible && !self.was_visible {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }
            self.was_visible = visible;
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if ctx.input(|i| i.viewport().minimized) == Some(true) {
            // Clear winit's minimized state before hiding so tray restore
            // doesn't immediately re-hide the window on the next frame.
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // No continuous animation, especially on software fallback. Controller
        // changes wake the UI immediately; this only detects external ShowWindow.
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
        #[cfg(not(windows))]
        if !self.shot_requested {
            if let Ok(path) = std::env::var("AION_SHOT") {
                self.shot_requested = true;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::ScreenshotCallback::new(move |image| {
                            std::fs::write(format!("{path}.raw"), image.as_raw()).unwrap();
                            std::fs::write(
                                format!("{path}.meta"),
                                format!("{} {}", image.width(), image.height()),
                            )
                            .unwrap();
                            std::process::exit(0);
                        }),
                    ));
                ui.ctx().request_repaint();
            }
        }
    }

    fn on_exit(&mut self) {
        self.ctrl.send(Cmd::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::UpdateInfo;
    use egui::{Event, Pos2, Rect, Shape, Vec2};
    use std::sync::mpsc::Receiver;

    // Standard Windows scales, custom fractional scales and stress cases.
    const SCALES: &[f32] = &[
        1.0, 1.1, 1.25, 1.37, 1.5, 1.75, 1.875, 2.0, 2.25, 2.5, 2.625, 3.0, 3.5, 4.0, 4.5, 4.75,
        5.0, 6.0, 8.0,
    ];

    struct Harness {
        ctx: egui::Context,
        app: App,
        commands: Receiver<Cmd>,
        time: f64,
        text: Vec<(String, Rect)>,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            theme::install_fonts(&ctx);
            theme::apply(&ctx);
            let (ctrl, commands) = Controller::for_ui_test_with_commands();
            Self {
                ctx,
                app: App::new(ctrl),
                commands,
                time: 0.0,
                text: Vec::new(),
            }
        }

        fn frame(&mut self, size: Vec2, scale: f32, events: Vec<Event>) {
            self.time += 1.0 / 30.0;
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(scale);
            let mut output = self.ctx.run_ui(input, |ui| self.app.show(ui));
            // No GPU in headless tests. Explicitly acknowledge texture updates.
            output.textures_delta.clear();
            assert_eq!(output.pixels_per_point, scale);
            self.text.clear();
            for clipped in &output.shapes {
                assert!(clipped.clip_rect.is_finite());
                if let Shape::Text(text) = &clipped.shape {
                    let bounds = Rect::from_min_size(text.pos, text.galley.size());
                    assert!(bounds.is_finite());
                    if bounds.intersects(clipped.clip_rect) {
                        // Below 300 points, horizontal scrolling is intentional.
                        assert!(
                            size.x < 300.0
                                || (bounds.left() >= -1.0 && bounds.right() <= size.x + 1.0),
                            "Text outside window at {size:?}, scale {scale}: {:?} {bounds:?}",
                            text.galley.text()
                        );
                        // Only click labels whose center is actually in the visible clip.
                        if clipped.clip_rect.contains(bounds.center()) {
                            self.text.push((text.galley.text().to_owned(), bounds));
                        }
                    }
                }
            }
            let meshes = self.ctx.tessellate(output.shapes, output.pixels_per_point);
            if size.x > 0.0 && size.y > 0.0 {
                assert!(!meshes.is_empty());
            }
        }

        fn find(&self, label: &str) -> Option<Pos2> {
            self.text
                .iter()
                .find(|(text, _)| text == label)
                .map(|(_, rect)| rect.center())
        }

        fn click(&mut self, label: &str, size: Vec2, scale: f32) {
            let pos = self
                .find(label)
                .unwrap_or_else(|| panic!("Missing visible control: {label}"));
            self.frame(size, scale, vec![Event::PointerMoved(pos)]);
            for pressed in [true, false] {
                self.frame(
                    size,
                    scale,
                    vec![Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    }],
                );
            }
        }

        fn wheel(&mut self, size: Vec2, scale: f32, delta: Vec2) {
            self.frame(
                size,
                scale,
                vec![
                    Event::PointerMoved((size * 0.5).to_pos2()),
                    Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta,
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                        modifiers: Default::default(),
                    },
                ],
            );
            // Let wheel smoothing settle before hit testing.
            for _ in 0..8 {
                self.frame(size, scale, vec![]);
            }
        }
    }

    #[test]
    fn layout_matrix_across_native_scales_themes_and_app_states() {
        let mut h = Harness::new();
        for theme in [egui::ThemePreference::Dark, egui::ThemePreference::Light] {
            h.ctx.set_theme(theme);
            for &scale in SCALES {
                for size in [
                    vec2(300.0, 240.0),
                    vec2(300.0, 1200.0),
                    vec2(1600.0, 240.0),
                    vec2(460.0, 680.0),
                    vec2(791.0, 600.0),
                    vec2(792.0, 600.0),
                    vec2(793.0, 600.0),
                    vec2(803.0, 600.0),
                    vec2(804.0, 600.0),
                    vec2(805.0, 600.0),
                    vec2(2048.0, 1280.0),
                    vec2(1280.0, 720.0) / scale,
                    vec2(2560.0, 1600.0) / scale,
                    vec2(3840.0, 2160.0) / scale,
                    vec2(5120.0, 2880.0) / scale,
                    vec2(7680.0, 4320.0) / scale,
                ] {
                    for state in 0..4 {
                        h.app.status.running = state == 1;
                        h.app.status.game_present = state == 1;
                        h.app.status.error = (state == 2).then(|| {
                            "Driver failure: ".to_owned()
                                + &"very-long-unbroken-path-component".repeat(20)
                        });
                        h.app.status.update = (state == 3).then(|| UpdateInfo {
                            version: "999.999.999".into(),
                            zip_url: String::new(),
                            manifest_url: String::new(),
                            state: UpdateState::Failed("Network error ".repeat(40)),
                        });
                        h.frame(size, scale, vec![]);
                    }
                }
            }
        }
    }

    #[test]
    fn controls_remain_clickable_after_mixed_dpi_and_resize_transitions() {
        let mut h = Harness::new();
        // Deliberately alternate high/low DPI and wide/narrow windows.
        for (size, scale) in [
            (vec2(900.0, 700.0), 1.0),
            (vec2(300.0, 240.0), 5.0),
            (vec2(1200.0, 800.0), 2.25),
            (vec2(460.0, 680.0), 8.0),
            (vec2(300.0, 240.0), 1.25),
        ] {
            h.frame(size, scale, vec![]);
            h.frame(size, scale, vec![]);
            h.click("Start engine", size, scale);
            assert!(matches!(
                h.commands.try_recv(),
                Ok(Cmd::Start { auto: false })
            ));
            h.app.status.running = true;
            h.frame(size, scale, vec![]);
            h.click("Stop engine", size, scale);
            assert!(matches!(
                h.commands.try_recv(),
                Ok(Cmd::Stop { manual: true })
            ));
            h.app.status.running = false;
            assert!(h.commands.try_recv().is_err(), "One command per click");
        }
    }

    #[test]
    fn scrolling_reaches_settings_and_long_errors_at_high_dpi() {
        for &scale in SCALES {
            let mut h = Harness::new();
            let size = vec2(300.0, 240.0);
            h.app.status.error = Some("Long error message. ".repeat(30));
            h.frame(size, scale, vec![]);
            for _ in 0..24 {
                if h.find("Auto-start with Aion").is_some() {
                    break;
                }
                h.wheel(size, scale, vec2(0.0, -64.0));
            }
            h.click("Auto-start with Aion", size, scale);
            assert!(matches!(h.commands.try_recv(), Ok(Cmd::SetWatch(true))));
            for _ in 0..24 {
                if h.find("Check for updates").is_some() {
                    break;
                }
                h.wheel(size, scale, vec2(0.0, -64.0));
            }
            h.click("Check for updates", size, scale);
            assert!(matches!(h.commands.try_recv(), Ok(Cmd::CheckUpdate)));
            for _ in 0..40 {
                if h.find("Dismiss").is_some() {
                    break;
                }
                h.wheel(size, scale, vec2(0.0, -128.0));
            }
            h.click("Dismiss", size, scale);
            assert!(h.app.dismissed_error.is_some());
            // A shrinking scrollable area must not leave the UI stranded
            // beyond its new content after an error is dismissed.
            h.wheel(size, scale, vec2(0.0, 10000.0));
            assert!(h.find("Start engine").is_some());
        }
    }
    #[test]
    fn small_display_at_extreme_scale_can_scroll_to_both_sides_of_controls() {
        let mut h = Harness::new();
        // 1280x720 at 800% leaves only 160x90 logical points.
        let size = vec2(160.0, 90.0);
        h.frame(size, 8.0, vec![]);
        h.wheel(size, 8.0, vec2(-80.0, 0.0));
        let mut saw_start = false;
        for _ in 0..40 {
            if h.find("Start engine").is_some() {
                saw_start = true;
                break;
            }
            h.wheel(size, 8.0, vec2(0.0, -16.0));
        }
        assert!(
            saw_start,
            "Engine button must be reachable on a tiny viewport"
        );
        h.click("Start engine", size, 8.0);
        assert!(matches!(
            h.commands.try_recv(),
            Ok(Cmd::Start { auto: false })
        ));
        // Horizontal wheel movement must also be possible; then resize back
        // to a normal width and ensure the old offset does not strand controls.
        h.wheel(size, 8.0, vec2(-1000.0, 0.0));
        h.frame(vec2(900.0, 900.0), 1.0, vec![]);
        h.frame(vec2(900.0, 900.0), 1.0, vec![]);
        assert!(h.find("Start engine").is_some());
        h.click("Check for updates", vec2(900.0, 900.0), 1.0);
        assert!(matches!(h.commands.try_recv(), Ok(Cmd::CheckUpdate)));
    }

    #[test]
    fn disabled_controls_and_restore_after_zero_sized_viewport() {
        let mut h = Harness::new();
        let size = vec2(460.0, 680.0);
        h.app.status.update = Some(UpdateInfo {
            version: "999.0.0".into(),
            zip_url: String::new(),
            manifest_url: String::new(),
            state: UpdateState::Downloading(40),
        });
        for &scale in SCALES {
            h.frame(size, scale, vec![]);
            h.frame(size, scale, vec![]);
            h.click("Start engine", size, scale);
            assert!(
                h.commands.try_recv().is_err(),
                "Busy updater must disable engine start"
            );
        }
        h.frame(Vec2::ZERO, 5.0, vec![]);
        h.app.status.update = None;
        h.frame(size, 1.25, vec![]);
        h.frame(size, 1.25, vec![]);
        h.click("Start engine", size, 1.25);
        assert!(matches!(
            h.commands.try_recv(),
            Ok(Cmd::Start { auto: false })
        ));
    }
    #[test]
    fn keyboard_focus_scrolls_controls_into_view_at_high_scaling() {
        let mut h = Harness::new();
        let size = vec2(300.0, 240.0);
        h.frame(size, 5.0, vec![]);
        for key in [
            egui::Key::Tab,
            egui::Key::Enter,
            egui::Key::Tab,
            egui::Key::Tab,
            egui::Key::Space,
        ] {
            for pressed in [true, false] {
                h.frame(
                    size,
                    5.0,
                    vec![Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Default::default(),
                    }],
                );
            }
            for _ in 0..8 {
                h.frame(size, 5.0, vec![]);
            }
            let focused = h
                .ctx
                .memory(|m| m.focused())
                .expect("Keyboard focus must remain on a control");
            let response = h.ctx.read_response(focused).unwrap();
            assert!(
                Rect::from_min_size(Pos2::ZERO, size).contains(response.rect.center()),
                "Focused control should scroll into view: {:?}",
                response.rect
            );
        }
        assert!(matches!(
            h.commands.try_recv(),
            Ok(Cmd::Start { auto: false })
        ));
        assert!(matches!(h.commands.try_recv(), Ok(Cmd::SetWatch(true))));
        assert!(h.commands.try_recv().is_err());
    }
}
