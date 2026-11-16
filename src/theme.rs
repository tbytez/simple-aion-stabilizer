//! Theme: dark/light palettes that follow the Windows app theme,
//! plus embedded Inter fonts.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Visuals};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub card: Color32,
    pub card_border: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent_a: Color32,
    pub off_a: Color32,
    pub danger: Color32,
    pub danger_bg: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(11, 14, 20),
            card: Color32::from_rgb(19, 24, 36),
            card_border: Color32::from_rgb(35, 42, 59),
            text: Color32::from_rgb(242, 244, 249),
            dim: Color32::from_rgb(138, 147, 166),
            accent_a: Color32::from_rgb(99, 102, 241),
            off_a: Color32::from_rgb(71, 85, 105),
            danger: Color32::from_rgb(248, 113, 113),
            danger_bg: Color32::from_rgb(60, 24, 30),
        }
    }

    pub fn light() -> Self {
        Self {
            bg: Color32::from_rgb(243, 245, 250),
            card: Color32::WHITE,
            card_border: Color32::from_rgb(224, 229, 240),
            text: Color32::from_rgb(23, 27, 38),
            dim: Color32::from_rgb(95, 104, 120),
            accent_a: Color32::from_rgb(79, 70, 229),
            off_a: Color32::from_rgb(148, 163, 184),
            danger: Color32::from_rgb(220, 38, 38),
            danger_bg: Color32::from_rgb(254, 226, 226),
        }
    }
}

/// Install Inter as the app font (falls back to egui defaults if absent).
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-md".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-Medium.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-sb".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")).into(),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".to_owned());
    fonts
        .families
        .insert(FontFamily::Name("md".into()), vec!["inter-md".to_owned()]);
    fonts
        .families
        .insert(FontFamily::Name("sb".into()), vec!["inter-sb".to_owned()]);
    ctx.set_fonts(fonts);
}

/// Install both themed styles and tell egui to follow the OS theme.
/// egui/winit tracks Windows dark/light changes automatically.
pub fn apply(ctx: &egui::Context) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        let pal = if theme == egui::Theme::Dark {
            Palette::dark()
        } else {
            Palette::light()
        };
        let mut v = if theme == egui::Theme::Dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        v.panel_fill = pal.bg;
        v.window_fill = pal.card;
        v.extreme_bg_color = pal.bg;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
        v.widgets.inactive.corner_radius = CornerRadius::same(8);
        v.widgets.hovered.corner_radius = CornerRadius::same(8);
        v.widgets.active.corner_radius = CornerRadius::same(8);
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = v;
        // Readable logical sizes; egui rasterizes these at each monitor's
        // native pixels-per-point rather than stretching a cached bitmap.
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
        style.spacing.interact_size = egui::vec2(40.0, 32.0);
        ctx.set_style_of(theme, style);
    }
    ctx.set_theme(egui::ThemePreference::System);
}

/// Palette for whatever theme is active right now.
pub fn palette(ctx: &egui::Context) -> Palette {
    if ctx.theme() == egui::Theme::Dark {
        Palette::dark()
    } else {
        Palette::light()
    }
}
