//! Custom-painted egui UI: frameless window, glowing power button, toggle
//! switches, dark/light palettes following the Windows theme.

use crate::controller::{Cmd, Controller, Status};
use crate::theme::{self, Palette};
use egui::{
    vec2, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Mesh, Pos2, Rect,
    Sense, Shape, Stroke, StrokeKind,
};
use std::time::{Duration, Instant};

const TITLE_H: f32 = 48.0;
const MARGIN: f32 = 16.0;

pub struct App {
    ctrl: Controller,
    status: Status,
    dismissed_error: Option<String>,
    frame: u32,
    start: Instant,
    #[cfg(not(windows))]
    shot: Option<String>,
}

impl App {
    pub fn new(ctrl: Controller) -> Self {
        Self {
            ctrl,
            status: Status::default(),
            dismissed_error: None,
            frame: 0,
            start: Instant::now(),
            #[cfg(not(windows))]
            shot: std::env::var("AION_SHOT").ok(),
        }
    }
}

impl eframe::App for App {
    /// Called before each ui() and also while the window is hidden (when a
    /// repaint was requested). All non-painting logic lives here.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.status = self.ctrl.status();
        self.frame = self.frame.wrapping_add(1);

        // close / minimize -> hide to tray
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if ctx.input(|i| i.viewport().minimized) == Some(true) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // repaint cadence: smooth pulse while running, idle poll otherwise
        ctx.request_repaint_after(if self.status.running {
            Duration::from_millis(40)
        } else {
            Duration::from_millis(900)
        });

        #[cfg(not(windows))]
        self.preview_hooks(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let pal = theme::palette(ui.ctx());
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, CornerRadius::ZERO, pal.bg);

        self.paint_title_bar(ui, rect, pal);
        let mut y = rect.min.y + TITLE_H + MARGIN;
        y = self.paint_hero(ui, rect, y, pal);
        if let Some(err) = self.visible_error() {
            y += 10.0;
            y = self.paint_error(ui, rect, y, err.to_string(), pal);
        }
        y += 12.0;
        let _ = self.paint_settings(ui, rect, y, pal);
        self.paint_footer(ui, rect, pal);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.ctrl.send(Cmd::Shutdown);
    }
}

impl App {
    fn visible_error(&self) -> Option<&str> {
        match (&self.status.error, &self.dismissed_error) {
            (Some(e), Some(d)) if e == d => None,
            (Some(e), _) => Some(e.as_str()),
            _ => None,
        }
    }

    // ---------------- title bar ----------------

    fn paint_title_bar(&self, ui: &mut egui::Ui, rect: Rect, pal: Palette) {
        let ctx = ui.ctx().clone();
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_H));
        ui.painter().rect_filled(bar, CornerRadius::ZERO, pal.title_bar);
        ui.painter().line_segment(
            [bar.left_bottom(), bar.right_bottom()],
            Stroke::new(1.0, pal.card_border),
        );

        // drag anywhere on the bar
        let drag = ui.interact(bar, ui.scope_id().with("drag"), Sense::drag());
        if drag.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        // logo dot + name
        let c = bar.left_center() + vec2(18.0, 0.0);
        let p = ui.painter();
        p.circle_filled(c, 9.0, pal.accent_a);
        p.circle_filled(c + vec2(2.0, -2.0), 5.5, pal.accent_b);
        p.circle_filled(c, 3.0, pal.bg);
        p.text(
            c + vec2(16.0, 0.0),
            Align2::LEFT_CENTER,
            "AION STABILIZER",
            FontId::new(11.5, FontFamily::Name("sb".into())),
            pal.dim,
        );

        // window buttons (both hide to tray: "-" and "x")
        let btn_w = 42.0;
        let close = Rect::from_min_size(
            Pos2::new(bar.max.x - btn_w, bar.min.y),
            vec2(btn_w, TITLE_H),
        );
        let min = Rect::from_min_size(
            Pos2::new(bar.max.x - btn_w * 2.0, bar.min.y),
            vec2(btn_w, TITLE_H),
        );

        let r_close = ui.interact(close, ui.scope_id().with("close"), Sense::click());
        let r_min = ui.interact(min, ui.scope_id().with("min"), Sense::click());
        for (r, is_close) in [(&r_close, true), (&r_min, false)] {
            if r.hovered() {
                let col = if is_close {
                    Color32::from_rgb(196, 43, 43)
                } else {
                    pal.btn_hover
                };
                ui.painter().rect_filled(r.rect, CornerRadius::ZERO, col);
            }
        }
        // glyphs
        let mc = min.center();
        ui.painter().line_segment(
            [mc + vec2(-5.0, 0.0), mc + vec2(5.0, 0.0)],
            Stroke::new(1.6, if r_min.hovered() { pal.text } else { pal.dim }),
        );
        let cc = close.center();
        let c_col = if r_close.hovered() {
            Color32::WHITE
        } else {
            pal.dim
        };
        ui.painter().line_segment(
            [cc + vec2(-4.5, -4.5), cc + vec2(4.5, 4.5)],
            Stroke::new(1.6, c_col),
        );
        ui.painter().line_segment(
            [cc + vec2(4.5, -4.5), cc + vec2(-4.5, 4.5)],
            Stroke::new(1.6, c_col),
        );
        if r_min.clicked() || r_close.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    // ---------------- hero card ----------------

    fn paint_hero(&mut self, ui: &mut egui::Ui, win: Rect, y: f32, pal: Palette) -> f32 {
        let ctx = ui.ctx().clone();
        let card = Rect::from_min_size(
            Pos2::new(win.min.x + MARGIN, y),
            vec2(win.width() - 2.0 * MARGIN, 258.0),
        );
        ui.painter()
            .rect_filled(card, CornerRadius::same(16), pal.card);
        ui.painter().rect_stroke(
            card,
            CornerRadius::same(16),
            Stroke::new(1.0, pal.card_border),
            StrokeKind::Inside,
        );

        let running = self.status.running;
        let cx = card.center().x;
        let cy = card.min.y + 96.0;
        let t = self.start.elapsed().as_secs_f32();

        // glow rings (running only, gentle pulse)
        if running {
            let pulse = 0.55 + 0.45 * (t * 1.7).sin();
            for i in 0..4u32 {
                let r = 58.0 + i as f32 * 11.0 + pulse * 5.0;
                let a = ((30 - i as i32 * 7).max(4) as f32) * (0.6 + 0.4 * pulse);
                let col = lerp(pal.on_a, pal.on_b, i as f32 / 4.0).gamma_multiply(a / 255.0);
                ui.painter()
                    .circle_stroke(Pos2::new(cx, cy), r, Stroke::new(10.0, col));
            }
        }

        // power button: gradient ring + inner disc + icon
        let btn = Rect::from_center_size(Pos2::new(cx, cy), vec2(112.0, 112.0));
        let resp = ui.allocate_rect(btn, Sense::click());
        let hover = resp.hovered();
        let down = resp.is_pointer_button_down_on();
        let scale = if down {
            0.96
        } else if hover {
            1.03
        } else {
            1.0
        };

        let (a, b) = if running {
            (pal.on_a, pal.on_b)
        } else {
            (pal.off_a, pal.off_b)
        };
        gradient_ring(
            ui.painter(),
            Pos2::new(cx, cy),
            52.0 * scale,
            60.0 * scale,
            a,
            b,
        );
        let inner = if running {
            lerp(pal.on_a, Color32::BLACK, 0.78)
        } else {
            lerp(pal.card, Color32::BLACK, 0.06)
        };
        ui.painter()
            .circle_filled(Pos2::new(cx, cy), 50.0 * scale, inner);

        let icon_col = if running {
            Color32::WHITE
        } else if hover {
            pal.text
        } else {
            pal.dim
        };
        power_icon(ui.painter(), Pos2::new(cx, cy), 20.0 * scale, icon_col);

        if hover {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.ctrl.send(Cmd::Toggle);
        }

        // status text
        let (title, sub) = if running {
            if self.status.auto_started {
                ("RUNNING", "auto-started with Aion 2")
            } else {
                ("RUNNING", "reshaping Aion 2 traffic")
            }
        } else {
            ("STOPPED", "tap the button to start")
        };
        ui.painter().text(
            Pos2::new(cx, card.min.y + 178.0),
            Align2::CENTER_CENTER,
            title,
            FontId::new(21.0, FontFamily::Name("sb".into())),
            if running { pal.on_b } else { pal.text },
        );
        ui.painter().text(
            Pos2::new(cx, card.min.y + 202.0),
            Align2::CENTER_CENTER,
            sub,
            FontId::new(12.0, FontFamily::Proportional),
            pal.dim,
        );

        // chips: game detection + mode
        let chip_y = card.min.y + 226.0;
        let (game_txt, game_dot) = if !self.status.watch {
            ("watch off", pal.dim)
        } else if self.status.game_present {
            ("Aion 2 running", pal.on_b)
        } else {
            ("Aion 2 idle", pal.dim)
        };
        let mode_txt = if self.status.auto_started && running {
            "AUTO"
        } else {
            "MANUAL"
        };
        let half = (card.width() - 12.0) / 2.0;
        self.chip(
            ui,
            Rect::from_min_size(Pos2::new(card.min.x + 8.0, chip_y), vec2(half, 24.0)),
            game_dot,
            game_txt,
            pal,
        );
        self.chip(
            ui,
            Rect::from_min_size(
                Pos2::new(card.min.x + 12.0 + half, chip_y),
                vec2(half - 8.0, 24.0),
            ),
            pal.accent_a,
            mode_txt,
            pal,
        );

        card.max.y
    }

    fn chip(&self, ui: &mut egui::Ui, rect: Rect, dot: Color32, text: &str, pal: Palette) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(12), pal.chip_bg);
        ui.painter().circle_filled(
            Pos2::new(rect.min.x + 14.0, rect.center().y),
            3.5,
            dot,
        );
        ui.painter().text(
            Pos2::new(rect.min.x + 24.0, rect.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::new(11.0, FontFamily::Name("md".into())),
            pal.dim,
        );
    }

    // ---------------- error banner ----------------

    fn paint_error(
        &mut self,
        ui: &mut egui::Ui,
        win: Rect,
        y: f32,
        msg: String,
        pal: Palette,
    ) -> f32 {
        let ctx = ui.ctx().clone();
        let rect = Rect::from_min_size(
            Pos2::new(win.min.x + MARGIN, y),
            vec2(win.width() - 2.0 * MARGIN, 40.0),
        );
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), pal.danger_bg);
        ui.painter().circle_stroke(
            Pos2::new(rect.min.x + 16.0, rect.center().y),
            6.0,
            Stroke::new(1.5, pal.danger),
        );
        ui.painter().text(
            Pos2::new(rect.min.x + 16.0, rect.center().y),
            Align2::CENTER_CENTER,
            "!",
            FontId::new(10.0, FontFamily::Name("sb".into())),
            pal.danger,
        );
        ui.painter().text(
            Pos2::new(rect.min.x + 30.0, rect.center().y),
            Align2::LEFT_CENTER,
            truncate(&msg, 42),
            FontId::new(11.0, FontFamily::Name("md".into())),
            pal.danger,
        );
        let x_btn = Rect::from_center_size(
            Pos2::new(rect.max.x - 18.0, rect.center().y),
            vec2(24.0, 24.0),
        );
        let r = ui.allocate_rect(x_btn, Sense::click());
        if r.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        ui.painter().text(
            x_btn.center(),
            Align2::CENTER_CENTER,
            "x",
            FontId::new(11.0, FontFamily::Name("sb".into())),
            if r.hovered() { pal.danger } else { pal.dim },
        );
        if r.clicked() {
            self.dismissed_error = self.status.error.clone();
        }
        rect.max.y
    }

    // ---------------- settings card ----------------

    fn paint_settings(&mut self, ui: &mut egui::Ui, win: Rect, y: f32, pal: Palette) -> f32 {
        let ctx = ui.ctx().clone();
        let rect = Rect::from_min_size(
            Pos2::new(win.min.x + MARGIN, y),
            vec2(win.width() - 2.0 * MARGIN, 178.0),
        );
        ui.painter()
            .rect_filled(rect, CornerRadius::same(16), pal.card);
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(16),
            Stroke::new(1.0, pal.card_border),
            StrokeKind::Inside,
        );
        ui.painter().text(
            Pos2::new(rect.min.x + 18.0, rect.min.y + 20.0),
            Align2::LEFT_CENTER,
            "SETTINGS",
            FontId::new(10.5, FontFamily::Name("sb".into())),
            pal.dim,
        );

        let rows: [(&str, &str, bool, Cmd, Cmd); 3] = [
            (
                "Auto-start with Aion 2",
                "Engine starts when the game launches",
                self.status.watch,
                Cmd::SetWatch(false),
                Cmd::SetWatch(true),
            ),
            (
                "Start engine on app open",
                "Turn on as soon as this app opens",
                self.status.startup_engine,
                Cmd::SetStartupEngine(false),
                Cmd::SetStartupEngine(true),
            ),
            (
                "Launch with Windows",
                "Start minimized in the tray on login",
                self.status.autostart,
                Cmd::SetAutostart(false),
                Cmd::SetAutostart(true),
            ),
        ];

        let mut ry = rect.min.y + 40.0;
        for (i, (title, sub, on, off_cmd, on_cmd)) in rows.into_iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(rect.min.x + 12.0, ry),
                vec2(rect.width() - 24.0, 42.0),
            );
            let hr = ui.interact(row, ui.scope_id().with(i), Sense::hover());
            if hr.hovered() {
                ui.painter()
                    .rect_filled(row, CornerRadius::same(10), pal.btn_hover);
            }
            ui.painter().text(
                Pos2::new(row.min.x + 8.0, row.min.y + 12.0),
                Align2::LEFT_CENTER,
                title,
                FontId::new(13.0, FontFamily::Name("md".into())),
                pal.text,
            );
            ui.painter().text(
                Pos2::new(row.min.x + 8.0, row.min.y + 29.0),
                Align2::LEFT_CENTER,
                sub,
                FontId::new(10.5, FontFamily::Proportional),
                pal.dim,
            );

            // toggle switch
            let sw = Rect::from_center_size(
                Pos2::new(row.max.x - 28.0, row.center().y),
                vec2(40.0, 22.0),
            );
            let r = ui.allocate_rect(sw, Sense::click());
            let anim = ctx.animate_bool(r.id, on);
            if r.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            let track = lerp(pal.off_a, pal.accent_a, anim);
            ui.painter().rect_filled(sw, CornerRadius::same(11), track);
            let kx = egui::emath::lerp(sw.min.x + 11.0..=sw.max.x - 11.0, anim);
            ui.painter()
                .circle_filled(Pos2::new(kx, sw.center().y), 8.0, Color32::WHITE);
            if r.clicked() {
                self.ctrl.send(if on { off_cmd } else { on_cmd });
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            ry += 42.0;
        }
        rect.max.y
    }

    // ---------------- footer ----------------

    fn paint_footer(&self, ui: &mut egui::Ui, win: Rect, pal: Palette) {
        let p = Pos2::new(win.center().x, win.max.y - 14.0);
        ui.painter().text(
            p,
            Align2::CENTER_CENTER,
            "minimize or close keeps it running in the tray",
            FontId::new(10.5, FontFamily::Proportional),
            pal.dim.gamma_multiply(0.75),
        );
    }

    // ---------------- preview helpers (non-windows) ----------------

    #[cfg(not(windows))]
    fn preview_hooks(&mut self, ctx: &egui::Context) {
        if self.frame == 3 && std::env::var("AION_PREVIEW_RUNNING").is_ok() {
            self.ctrl.send(Cmd::Start { auto: false });
        }
        if std::env::var("AION_PREVIEW_WATCH").is_ok() {
            self.ctrl.send(Cmd::SetWatch(true));
        }
        if let Some(path) = self.shot.clone() {
            if self.frame == 10 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::ScreenshotCallback::new(move |img| {
                        let _ = std::fs::write(format!("{path}.raw"), img.as_raw());
                        let _ = std::fs::write(
                            format!("{path}.meta"),
                            format!("{} {}", img.width(), img.height()),
                        );
                    }),
                ));
            }
            if self.frame == 16 {
                std::process::exit(0);
            }
        }
    }
}

// ---------------- paint helpers ----------------

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
    )
}

/// Ring mesh with a gradient from `a` to `b` around the circle.
fn gradient_ring(
    painter: &egui::Painter,
    center: Pos2,
    inner_r: f32,
    outer_r: f32,
    a: Color32,
    b: Color32,
) {
    const N: usize = 72;
    let mut mesh = Mesh::default();
    for i in 0..=N {
        let f = i as f32 / N as f32;
        let ang = f * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let dir = vec2(ang.cos(), ang.sin());
        let col = lerp(a, b, 0.5 + 0.5 * (f * std::f32::consts::TAU).cos());
        mesh.colored_vertex(center + dir * outer_r, col);
        mesh.colored_vertex(center + dir * inner_r, col);
    }
    for i in 0..N {
        let o = (i * 2) as u32;
        mesh.add_triangle(o, o + 1, o + 2);
        mesh.add_triangle(o + 1, o + 3, o + 2);
    }
    painter.add(Shape::mesh(mesh));
}

/// Classic power symbol: arc with a gap at top + vertical stroke.
fn power_icon(painter: &egui::Painter, center: Pos2, r: f32, col: Color32) {
    let w = r * 0.22;
    // arc: gap at top, spans 300deg around the bottom
    let mut pts = Vec::with_capacity(64);
    let start = 120.0f32.to_radians();
    let end = 420.0f32.to_radians();
    for i in 0..64 {
        let a = start + (end - start) * (i as f32 / 63.0);
        pts.push(center + vec2(a.cos() * r, -a.sin() * r));
    }
    painter.add(Shape::line(pts.clone(), Stroke::new(w, col)));
    if let (Some(f), Some(l)) = (pts.first(), pts.last()) {
        painter.circle_filled(*f, w / 2.0, col);
        painter.circle_filled(*l, w / 2.0, col);
    }
    // vertical stroke from top center downward
    let top = center + vec2(0.0, -r * 1.15);
    let bot = center + vec2(0.0, -r * 0.1);
    painter.line_segment([top, bot], Stroke::new(w, col));
    painter.circle_filled(top, w / 2.0, col);
    painter.circle_filled(bot, w / 2.0, col);
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n - 3).collect::<String>())
    }
}
