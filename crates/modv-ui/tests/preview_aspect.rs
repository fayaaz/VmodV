//! `preview_panel` must letterbox the live output inside its available area
//! instead of stretching non-16:9 sources.

use egui::{Rect, pos2, vec2};
use modv_ui::shell::fit_preview_rect;

fn assert_rect(actual: Rect, min: [f32; 2], max: [f32; 2]) {
    assert!(
        (actual.min.x - min[0]).abs() < 0.01
            && (actual.min.y - min[1]).abs() < 0.01
            && (actual.max.x - max[0]).abs() < 0.01
            && (actual.max.y - max[1]).abs() < 0.01,
        "expected ({min:?})..({max:?}), got {:?}",
        actual
    );
}

#[test]
fn sixteen_by_nine_fills_the_available_area() {
    let rect = fit_preview_rect(vec2(640.0, 360.0), [1920, 1080]);
    assert_rect(rect, [0.0, 0.0], [640.0, 360.0]);
}

#[test]
fn four_by_three_is_pillarboxed_centered() {
    let rect = fit_preview_rect(vec2(640.0, 360.0), [640, 480]);
    assert_rect(rect, [80.0, 0.0], [560.0, 360.0]);
}

#[test]
fn twenty_one_by_nine_is_letterboxed_centered() {
    let rect = fit_preview_rect(vec2(640.0, 360.0), [2560, 1080]);
    assert_rect(rect, [0.0, 45.0], [640.0, 315.0]);
}

#[test]
fn portrait_output_is_pillarboxed_and_not_cropped() {
    let rect = fit_preview_rect(vec2(640.0, 360.0), [1080, 1920]);
    assert_rect(rect, [218.75, 0.0], [421.25, 360.0]);
}

#[test]
fn square_output_is_pillarboxed() {
    let rect = fit_preview_rect(vec2(640.0, 360.0), [512, 512]);
    assert_rect(rect, [140.0, 0.0], [500.0, 360.0]);
}

#[test]
fn fitting_uses_the_tighter_axis() {
    // A tall panel: the height is the limiting dimension.
    let rect = fit_preview_rect(vec2(300.0, 500.0), [640, 480]);
    assert_rect(rect, [0.0, 137.5], [300.0, 362.5]);

    // A wide panel: the width is the limiting dimension.
    let rect = fit_preview_rect(vec2(1000.0, 300.0), [1920, 1080]);
    assert_rect(rect, [233.3333, 0.0], [766.6667, 300.0]);
}

#[test]
fn unknown_or_degenerate_resolution_falls_back_to_sixteen_by_nine() {
    let expected = Rect::from_min_max(pos2(0.0, 0.0), pos2(640.0, 360.0));
    assert_eq!(fit_preview_rect(vec2(640.0, 360.0), [0, 0]), expected);
    assert_eq!(fit_preview_rect(vec2(640.0, 360.0), [960, 0]), expected);
    assert_eq!(fit_preview_rect(vec2(640.0, 360.0), [0, 540]), expected);
}

#[test]
fn fitted_rect_never_overflows_the_available_area() {
    for resolution in [
        [1920u32, 1080],
        [640, 480],
        [2560, 1080],
        [1080, 1920],
        [512, 512],
        [3840, 2160],
        [1, 1],
    ] {
        let available = vec2(320.0, 180.0);
        let rect = fit_preview_rect(available, resolution);
        assert!(rect.min.x >= -0.01 && rect.min.y >= -0.01);
        assert!(rect.max.x <= available.x + 0.01);
        assert!(rect.max.y <= available.y + 0.01);
        assert!(rect.width() > 0.0 && rect.height() > 0.0);

        let source_aspect = resolution[0] as f32 / resolution[1] as f32;
        assert!(
            (rect.aspect_ratio() - source_aspect).abs() < 0.001,
            "resolution {resolution:?} should keep aspect {source_aspect}, got {}",
            rect.aspect_ratio()
        );
    }
}
