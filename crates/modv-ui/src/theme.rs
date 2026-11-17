//! The native shell's black-and-neon-green theme.
//!
//! The palette descends from the legacy dark chrome (`App.vue` +
//! `golden-layout_theme.css`) but is rebuilt as near-black surfaces with a
//! single neon-green accent: no grey chrome, no rounded pills. Fonts are the
//! bundled IBM Plex faces ([`crate::fonts`]), one visible step larger than the
//! original port.
//!
//! Surfaces are deliberately separated so a control can never disappear into
//! whatever it sits on: `BACKGROUND`/`PANEL`/`PANEL_ALT`/`INPUT` form a strictly
//! ordered ladder, and sliders get their own track that differs from all of
//! them ([`SLIDER_TRACK`]) plus an accent trailing fill.

use egui::style::HandleShape;
use egui::{Color32, CornerRadius, Stroke};

/// Window/background fill (`#050806`, near-black with a green cast).
pub const BACKGROUND: Color32 = Color32::from_rgb(5, 8, 6);
/// Panel fill (`#0A100C`): side panels, dock column, tab strip, floating windows.
pub const PANEL: Color32 = Color32::from_rgb(10, 16, 12);
/// Raised surface inside a panel (`#0E1711`): banners, selected rows, badges.
/// Never use this as a panel/window background.
pub const PANEL_ALT: Color32 = Color32::from_rgb(14, 23, 17);
/// Input/track well (`#040705`).
pub const INPUT: Color32 = Color32::from_rgb(4, 7, 5);
/// Foreground text (`#E8F5EC`).
pub const FOREGROUND: Color32 = Color32::from_rgb(232, 245, 236);
/// Muted/secondary text (`#7E9B88`).
pub const MUTED: Color32 = Color32::from_rgb(126, 155, 136);
/// Focus/hover accent, neon green (`#00E676`).
pub const ACCENT: Color32 = Color32::from_rgb(0, 230, 118);
/// A dimmed accent for selected backgrounds (`#0B3D25`).
pub const ACCENT_DIM: Color32 = Color32::from_rgb(11, 61, 37);
/// Border/divider (`#1C3A28`).
pub const BORDER: Color32 = Color32::from_rgb(28, 58, 40);
/// Error/red (`#FF4D4D`).
pub const DANGER: Color32 = Color32::from_rgb(255, 77, 77);
/// Warning/amber (`#FFB300`).
pub const WARNING: Color32 = Color32::from_rgb(255, 179, 0);
/// Success/OK: the accent is the success colour in this palette.
pub const OK: Color32 = ACCENT;
/// Tween badge blue (`#5E9CFF`): semantic, deliberately outside the palette.
pub const TWEEN: Color32 = Color32::from_rgb(94, 156, 255);

/// The tab strip's fill. Same as [`PANEL`] so a detached tab never changes
/// surface relative to the strip it came from.
pub const STRIP_FILL: Color32 = PANEL;
/// The dock column's fill; matches the strip for the same reason.
pub const DOCK_FILL: Color32 = PANEL;
/// A slider's rail (`#143124`): darker than the panel but clearly greener,
/// and distinct from every surface ladder entry.
pub const SLIDER_TRACK: Color32 = Color32::from_rgb(20, 49, 36);
/// The filled part of a slider rail.
pub const SLIDER_TRAIL: Color32 = ACCENT;
/// The slider handle outline; the fill is the rail colour, so the bright
/// stroke is what marks the handle.
pub const SLIDER_HANDLE: Color32 = FOREGROUND;

/// Install the fonts, palette, square widget chrome and type scale on `ctx`.
pub fn apply(ctx: &egui::Context) {
    crate::fonts::install(ctx);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = INPUT;
    visuals.faint_bg_color = PANEL_ALT;
    visuals.override_text_color = Some(FOREGROUND);
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.handle_shape = HandleShape::Rect { aspect_ratio: 0.45 };
    visuals.slider_trailing_fill = true;

    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, FOREGROUND);
    visuals.widgets.inactive.bg_fill = PANEL_ALT;
    visuals.widgets.inactive.weak_bg_fill = PANEL_ALT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, FOREGROUND);
    visuals.widgets.hovered.bg_fill = BORDER;
    visuals.widgets.hovered.weak_bg_fill = BORDER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, FOREGROUND);
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, ACCENT);
    // An active floating window's title bar paints `widgets.open.weak_bg_fill`.
    visuals.widgets.open.bg_fill = PANEL_ALT;
    visuals.widgets.open.weak_bg_fill = PANEL_ALT;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, ACCENT);

    // Sharper look: every window, menu and widget is a square rectangle.
    visuals.window_corner_radius = CornerRadius::same(0);
    visuals.menu_corner_radius = CornerRadius::same(0);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(0);
    }

    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 5.0);
        style.spacing.button_padding = egui::vec2(9.0, 4.0);
        style.spacing.interact_size.y = 22.0;
        style.spacing.slider_width = 140.0;
        style.spacing.slider_rail_height = 9.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(
                20.0,
                egui::FontFamily::Name(crate::fonts::SANS_SEMIBOLD_FAMILY.into()),
            ),
        );
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(15.0));
    });
}

/// Give one slider its own high-contrast rail, trailing fill and handle.
///
/// egui paints a slider's rail with `widgets.inactive.bg_fill` — on the old
/// theme that was the same grey as a docked panel, so the rail vanished. This
/// override is scoped to the slider (and whatever is inside the same scope)
/// rather than changing the global widget fills: buttons keep their panel-alt
/// surface while the rail gets [`SLIDER_TRACK`].
pub fn slider_visuals(ui: &mut egui::Ui) {
    let visuals = ui.visuals_mut();
    visuals.widgets.inactive.bg_fill = SLIDER_TRACK;
    visuals.widgets.inactive.weak_bg_fill = SLIDER_TRACK;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.5, SLIDER_HANDLE);
    visuals.widgets.hovered.bg_fill = ACCENT_DIM;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, ACCENT);
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, INPUT);
    visuals.selection.bg_fill = SLIDER_TRAIL;
    visuals.selection.stroke = Stroke::new(1.0, SLIDER_HANDLE);
    visuals.handle_shape = HandleShape::Rect { aspect_ratio: 0.45 };
    visuals.slider_trailing_fill = true;
}

/// Draw a slider with the themed rail and handle.
pub fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    ui.scope(|ui| {
        slider_visuals(ui);
        ui.add(slider)
    })
    .inner
}

/// A small square badge used for expression/tween/status markers.
pub fn badge(ui: &mut egui::Ui, text: &str, colour: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(0), colour.gamma_multiply(0.22));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::monospace(11.0),
        colour,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    /// WCAG relative luminance.
    fn luminance(colour: Color32) -> f32 {
        0.2126 * channel(colour.r()) + 0.7152 * channel(colour.g()) + 0.0722 * channel(colour.b())
    }

    /// WCAG contrast ratio between two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn apply_installs_the_squared_chrome_and_the_bigger_type_scale() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let style = ctx.style_of(egui::Theme::Dark);
        let visuals = &style.visuals;
        assert_eq!(visuals.window_corner_radius, CornerRadius::same(0));
        assert_eq!(visuals.menu_corner_radius, CornerRadius::same(0));
        assert_eq!(
            visuals.widgets.inactive.corner_radius,
            CornerRadius::same(0)
        );
        assert_eq!(visuals.widgets.hovered.corner_radius, CornerRadius::same(0));
        assert_eq!(visuals.widgets.active.corner_radius, CornerRadius::same(0));
        assert_eq!(visuals.widgets.open.weak_bg_fill, PANEL_ALT);
        assert_eq!(visuals.panel_fill, BACKGROUND);
        assert_eq!(visuals.window_fill, PANEL);
        assert_eq!(
            style.text_styles[&egui::TextStyle::Body],
            egui::FontId::proportional(16.0)
        );
        assert_eq!(
            style.text_styles[&egui::TextStyle::Button],
            egui::FontId::proportional(15.0)
        );
        assert_eq!(
            style.text_styles[&egui::TextStyle::Monospace],
            egui::FontId::monospace(15.0)
        );
        assert_eq!(style.spacing.interact_size.y, 22.0);
        assert_eq!(style.spacing.slider_width, 140.0);
    }

    #[test]
    fn the_accent_clears_aa_contrast_on_the_background() {
        let ratio = contrast(ACCENT, BACKGROUND);
        assert!(
            ratio >= 4.5,
            "neon green on near-black must clear WCAG AA, got {ratio:.2}"
        );
        assert!(
            contrast(FOREGROUND, PANEL) >= 7.0,
            "body text must clear AAA on panels"
        );
        assert!(
            contrast(MUTED, PANEL) >= 4.5,
            "muted text must still clear AA on panels"
        );
    }

    #[test]
    fn the_surface_ladder_is_strictly_ordered() {
        let surfaces = [INPUT, BACKGROUND, PANEL, PANEL_ALT];
        for pair in surfaces.windows(2) {
            assert!(
                luminance(pair[0]) < luminance(pair[1]),
                "surfaces must get lighter in ladder order: {:?} vs {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn the_slider_track_differs_from_every_panel_and_background() {
        for surface in [BACKGROUND, PANEL, PANEL_ALT, INPUT] {
            assert_ne!(
                SLIDER_TRACK, surface,
                "the slider rail must not match a surface it can sit on"
            );
            assert!(
                contrast(SLIDER_TRACK, surface) > 1.1,
                "the rail must be visibly distinct from {surface:?}"
            );
        }
        assert_eq!(SLIDER_TRAIL, ACCENT, "the filled rail is the accent");
        assert_ne!(SLIDER_HANDLE, SLIDER_TRACK, "the handle must read");
        assert!(
            contrast(SLIDER_HANDLE, SLIDER_TRACK) >= 4.5,
            "the handle stroke must clear AA against the rail"
        );
    }

    #[test]
    fn the_dock_and_the_tab_strip_share_one_fill() {
        assert_eq!(DOCK_FILL, STRIP_FILL);
        assert_eq!(DOCK_FILL, PANEL);
        assert_ne!(PANEL_ALT, DOCK_FILL, "PANEL_ALT is a surface, not a panel");
    }

    #[test]
    fn semantic_colours_stay_outside_the_green_ladder() {
        assert_ne!(DANGER, ACCENT);
        assert_ne!(WARNING, ACCENT);
        assert_eq!(OK, ACCENT);
        assert_ne!(TWEEN, ACCENT);
    }
}
