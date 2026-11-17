//! The dark theme, ported from `App.vue` + `golden-layout_theme.css`.
//!
//! Legacy palette: `#111` background, white foreground, `#f1c410` focus
//! accent, `#2d2d2d` chrome, 12px Inter / IAW Mono. P0 uses egui's bundled
//! fonts; P1 replaces them with the bundled Inter/IAW Mono faces (R8).

use egui::{Color32, CornerRadius, Stroke};

/// Window/background fill (`--background-color`, `#111111`).
pub const BACKGROUND: Color32 = Color32::from_rgb(17, 17, 17);
/// Panel fill (`#151515`).
pub const PANEL: Color32 = Color32::from_rgb(21, 21, 21);
/// Slightly raised panel/chrome (`#2d2d2d`).
pub const PANEL_ALT: Color32 = Color32::from_rgb(45, 45, 45);
/// Input/track fill.
pub const INPUT: Color32 = Color32::from_rgb(10, 10, 10);
/// Foreground text.
pub const FOREGROUND: Color32 = Color32::from_rgb(238, 238, 238);
/// Muted/secondary text.
pub const MUTED: Color32 = Color32::from_rgb(150, 150, 150);
/// Focus accent (`--focus-color`, `#f1c410`).
pub const ACCENT: Color32 = Color32::from_rgb(241, 196, 16);
/// A dimmed accent for selected backgrounds.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(78, 64, 12);
/// Border/divider.
pub const BORDER: Color32 = Color32::from_rgb(58, 58, 58);
/// Error/red.
pub const DANGER: Color32 = Color32::from_rgb(238, 39, 17);
/// Success/green.
pub const OK: Color32 = Color32::from_rgb(18, 192, 91);
/// Tween badge blue.
pub const TWEEN: Color32 = Color32::from_rgb(94, 156, 255);

/// Install the modV theme on `ctx`.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = INPUT;
    visuals.faint_bg_color = PANEL_ALT;
    visuals.override_text_color = Some(FOREGROUND);
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.bg_fill = PANEL_ALT;
    visuals.widgets.inactive.weak_bg_fill = PANEL_ALT;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(58, 58, 58);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(58, 58, 58);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.window_corner_radius = CornerRadius::same(0);
    visuals.menu_corner_radius = CornerRadius::same(2);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = 20.0;
        style.spacing.slider_width = 120.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(10.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(11.0));
    });
}

/// A small rounded badge used for expression/tween/status markers.
pub fn badge(ui: &mut egui::Ui, text: &str, colour: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), colour.gamma_multiply(0.22));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::monospace(9.0),
        colour,
    );
    response
}
