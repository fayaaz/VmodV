//! Same-tab output presentation policy.
//!
//! The browser shell's `Open output` no longer opens a popup that plays a
//! `captureStream`: browser video encoding softens the image and adds a frame
//! of latency. Instead the shell draws the compositor's main texture over the
//! whole canvas in the same tab, hides the UI and renders at the full viewport
//! size. `F` toggles the browser's fullscreen mode and a cog revealed by the
//! top-right corner returns to the UI.
//!
//! Everything here is platform-neutral so the state machine and geometry can
//! be unit-tested on the host; `app.rs` owns the egui/fullscreen wiring.

/// The UI keeps its internal render target at the native reference width.
pub const UI_MAX_WIDTH: u32 = 960;

/// The output view's longest edge is capped at 4K. A high-DPI browser window
/// can expose an 8K physical viewport; that would allocate enormous group
/// targets for no visible gain.
pub const OUTPUT_MAX_DIMENSION: u32 = 3840;

/// The cog's inset from the top-right viewport corner, in points.
pub const COG_INSET: f32 = 12.0;

/// The cog button's diameter, in points.
pub const COG_SIZE: f32 = 36.0;

/// The top-right corner zone that reveals the cog, in points.
pub const COG_REVEAL_SIZE: f32 = 96.0;

/// Whether the output view is showing and whether it is fullscreen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputView {
    active: bool,
    fullscreen: bool,
}

impl OutputView {
    pub const fn new() -> Self {
        Self {
            active: false,
            fullscreen: false,
        }
    }

    /// Whether the shell should draw the output instead of the UI.
    pub const fn is_active(&self) -> bool {
        self.active
    }

    pub const fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Enter the output view. Returns `true` when this call opened it.
    pub fn open(&mut self) -> bool {
        let opened = !self.active;
        self.active = true;
        opened
    }

    /// Leave the output view, dropping fullscreen. Returns `true` when it was
    /// open.
    pub fn close(&mut self) -> bool {
        let was_active = self.active;
        self.active = false;
        self.fullscreen = false;
        was_active
    }

    /// Toggle fullscreen. Returns the resulting state. Only meaningful while
    /// the output view is open.
    pub fn toggle_fullscreen(&mut self) -> bool {
        if self.active {
            self.fullscreen = !self.fullscreen;
        }
        self.fullscreen
    }

    /// Set fullscreen from the browser's real fullscreen state.
    pub fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen = self.active && on;
    }
}

/// The compositor's internal render-target size.
///
/// The UI view is capped at [`UI_MAX_WIDTH`] to keep a 4K canvas affordable.
/// The output view renders at the real viewport size: it is the performance
/// output, so it must not be softened by the UI budget.
pub fn render_target_size(width: u32, height: u32, output: bool) -> [u32; 2] {
    let width = width.max(1);
    let height = height.max(1);
    if output {
        let longest = width.max(height);
        if longest <= OUTPUT_MAX_DIMENSION {
            return [width, height];
        }
        let scale = f64::from(OUTPUT_MAX_DIMENSION) / f64::from(longest);
        return [
            ((f64::from(width) * scale).round() as u32).max(1),
            ((f64::from(height) * scale).round() as u32).max(1),
        ];
    }
    if width <= UI_MAX_WIDTH {
        [width, height]
    } else {
        let scaled_height = (u64::from(height) * u64::from(UI_MAX_WIDTH) / u64::from(width)) as u32;
        [UI_MAX_WIDTH, scaled_height.max(1)]
    }
}

/// Fit `source` inside `available`, preserving its aspect ratio.
pub fn fit_size(available: [f32; 2], source: [u32; 2]) -> [f32; 2] {
    let width = source[0].max(1) as f32;
    let height = source[1].max(1) as f32;
    let scale = (available[0].max(0.0) / width).min(available[1].max(0.0) / height);
    [width * scale, height * scale]
}

/// Whether a pointer position reveals the return-to-UI cog.
pub fn cog_revealed(pointer: Option<[f32; 2]>, viewport: [f32; 2]) -> bool {
    let Some([x, y]) = pointer else {
        return false;
    };
    let width = viewport[0].max(0.0);
    let height = viewport[1].max(0.0);
    let min_x = (width - COG_REVEAL_SIZE).max(0.0);
    let max_y = COG_REVEAL_SIZE.min(height);
    (min_x..=width).contains(&x) && (0.0..=max_y).contains(&y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: [f32; 2], expected: [f32; 2]) {
        assert!(
            (actual[0] - expected[0]).abs() < 0.01 && (actual[1] - expected[1]).abs() < 0.01,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn output_view_opens_and_closes() {
        let mut view = OutputView::new();
        assert!(!view.is_active());
        assert!(view.open());
        assert!(view.is_active());
        assert!(!view.open());
        assert!(view.close());
        assert!(!view.is_active());
        assert!(!view.close());
    }

    #[test]
    fn closing_output_view_leaves_fullscreen() {
        let mut view = OutputView::new();
        view.open();
        assert!(view.toggle_fullscreen());
        assert!(view.is_fullscreen());
        view.close();
        assert!(!view.is_fullscreen());
    }

    #[test]
    fn fullscreen_only_applies_while_the_output_view_is_open() {
        let mut view = OutputView::new();
        assert!(!view.toggle_fullscreen());
        view.set_fullscreen(true);
        assert!(!view.is_fullscreen());

        view.open();
        view.set_fullscreen(true);
        assert!(view.is_fullscreen());
        view.set_fullscreen(false);
        assert!(!view.is_fullscreen());
    }

    #[test]
    fn ui_render_target_stays_at_the_960_budget() {
        assert_eq!(render_target_size(3840, 2160, false), [960, 540]);
        assert_eq!(render_target_size(1280, 800, false), [960, 600]);
        assert_eq!(render_target_size(800, 600, false), [800, 600]);
    }

    #[test]
    fn output_view_renders_at_the_full_viewport() {
        assert_eq!(render_target_size(3840, 2160, true), [3840, 2160]);
        assert_eq!(render_target_size(1280, 720, true), [1280, 720]);
        assert_eq!(render_target_size(800, 600, true), [800, 600]);
    }

    #[test]
    fn output_view_is_capped_at_four_k() {
        assert_eq!(render_target_size(7680, 4320, true), [3840, 2160]);
        assert_eq!(render_target_size(5120, 1440, true), [3840, 1080]);
    }

    #[test]
    fn render_target_never_collapses_to_zero() {
        assert_eq!(render_target_size(0, 0, false), [1, 1]);
        assert_eq!(render_target_size(1920, 0, false), [960, 1]);
    }

    #[test]
    fn fit_size_letterboxes_wide_output() {
        assert_close(fit_size([100.0, 100.0], [1920, 1080]), [100.0, 56.25]);
    }

    #[test]
    fn fit_size_pillarboxes_tall_output() {
        assert_close(fit_size([100.0, 100.0], [1080, 1920]), [56.25, 100.0]);
    }

    #[test]
    fn cog_reveals_only_in_the_top_right_hot_zone() {
        let viewport = [1280.0, 720.0];
        assert!(cog_revealed(Some([1270.0, 20.0]), viewport));
        assert!(cog_revealed(Some([1200.0, 80.0]), viewport));
        assert!(!cog_revealed(Some([640.0, 360.0]), viewport));
        assert!(!cog_revealed(Some([1270.0, 700.0]), viewport));
        assert!(!cog_revealed(None, viewport));
    }
}
