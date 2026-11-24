//! One-shot startup splash: the VmodV logo alone on black, fading out with its
//! backdrop to reveal the app.
//!
//! This is deliberately separate from the About dialog: the splash needs no
//! interaction, closes itself, and leaves no persisted state. `UiState::new()`
//! starts with no splash, so the harnesses that build a default state keep
//! rendering the ordinary shell.
//!
//! The logo is the shipped `resources/icon.png`, decoded once and uploaded as
//! an egui texture. Drawing it with a `layer_painter` keeps the splash above
//! every panel without stealing input from the app starting up behind it.

use std::time::Duration;

use crate::state::UiState;
use egui::{Color32, ColorImage, Order, TextureOptions, vec2};

/// How long the logo stays fully opaque before the fade begins (seconds).
pub const SPLASH_HOLD_SECONDS: f64 = 0.6;
/// How long the fade itself takes (seconds).
pub const SPLASH_FADE_SECONDS: f64 = 1.0;
/// When the splash is fully transparent and closes itself (seconds).
pub const SPLASH_TOTAL_SECONDS: f64 = SPLASH_HOLD_SECONDS + SPLASH_FADE_SECONDS;

/// The one-shot splash's clock. `started_at` is stamped on the first frame so
/// the app does not have to know the egui context's time origin.
#[derive(Clone, Copy, Debug, Default)]
pub struct SplashState {
    pub started_at: Option<f64>,
}

/// Logo opacity at `elapsed` seconds after the splash started.
///
/// Fully opaque through the hold, then a linear ramp down to zero. All callers
/// (and tests) share this one curve, so the painted alpha and the close time
/// cannot drift apart.
pub fn splash_alpha(elapsed: f64) -> f32 {
    if elapsed <= SPLASH_HOLD_SECONDS {
        1.0
    } else if elapsed >= SPLASH_TOTAL_SECONDS {
        0.0
    } else {
        ((SPLASH_TOTAL_SECONDS - elapsed) / SPLASH_FADE_SECONDS) as f32
    }
}

/// Draw the splash for this frame, advancing its clock and closing it once the
/// fade completes. Does nothing when no splash was requested.
pub fn draw(ctx: &egui::Context, state: &mut UiState) {
    let Some(splash) = state.splash.as_mut() else {
        return;
    };

    let now = ctx.input(|input| input.time);
    let started = *splash.started_at.get_or_insert(now);
    let alpha = splash_alpha((now - started).max(0.0));
    if alpha <= 0.0 {
        state.splash = None;
        return;
    }

    // Keep the fade moving on a quiet app without forcing an extra pass in the
    // same frame (`request_repaint` would advance egui's clock twice there).
    ctx.request_repaint_after(Duration::from_millis(16));

    if state.splash_logo.is_none() {
        state.splash_logo = Some(load_logo(ctx));
    }
    let Some(logo) = state.splash_logo.as_ref() else {
        return;
    };

    let screen = ctx.viewport_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        Order::Debug,
        egui::Id::unique("modv-splash"),
    ));
    painter.rect_filled(
        screen,
        0.0,
        Color32::from_black_alpha((255.0 * alpha).round() as u8),
    );

    let size = (screen.width().min(screen.height()) * 0.32).clamp(120.0, 280.0);
    let rect = egui::Rect::from_center_size(screen.center(), vec2(size, size));
    painter.image(
        logo.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::from_white_alpha((255.0 * alpha).round() as u8),
    );
}

/// Decode the shipped app icon once and upload a scaled RGBA copy to egui.
fn load_logo(ctx: &egui::Context) -> egui::TextureHandle {
    const LOGO: &[u8] = include_bytes!("../../../resources/icon.png");
    const SIDE: u32 = 256;

    let image = image::load_from_memory(LOGO)
        .expect("resources/icon.png is a valid PNG")
        .resize(SIDE, SIDE, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let pixels = ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    ctx.load_texture("vmodv-splash-logo", pixels, TextureOptions::LINEAR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacity_is_one_through_the_hold_and_zero_after_the_fade() {
        assert_eq!(splash_alpha(0.0), 1.0);
        assert_eq!(splash_alpha(SPLASH_HOLD_SECONDS), 1.0);
        assert_eq!(splash_alpha(SPLASH_TOTAL_SECONDS), 0.0);
        assert_eq!(splash_alpha(SPLASH_TOTAL_SECONDS * 2.0), 0.0);
    }

    #[test]
    fn opacity_only_decreases_during_the_fade() {
        let mut previous = 1.0;
        for step in 0..=20 {
            let elapsed = SPLASH_HOLD_SECONDS + SPLASH_FADE_SECONDS * f64::from(step) / 20.0;
            let alpha = splash_alpha(elapsed);
            assert!(alpha <= previous, "alpha must not increase at {elapsed}s");
            previous = alpha;
        }
        assert_eq!(previous, 0.0);
    }
}
