//! P0 kittest smoke tests: every probe must render without panicking and
//! expose its accessibility labels.
//!
//! These are interaction/smoke tests, not golden images. The lavapipe and
//! reference-GPU frame traces are the separate P0 performance evidence.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::widgets::RangeControl;
use modv_ui::{EngineSnapshot, UiState};

#[test]
fn range_control_renders_and_exposes_its_label() {
    let mut control = RangeControl::new("gain", 0.5, 0.0, 2.0, 0.001)
        .with_unit("dB")
        .with_badges(true, true);
    let mut harness = Harness::builder()
        .with_accessibility_check(false)
        .build_ui(move |ui| {
            control.draw(ui);
        });
    harness.run();
    // The numeric field is a real egui DragValue; the label and badges are
    // custom-painted, so they are asserted through the pure logic tests.
}

#[test]
fn shell_draws_with_an_empty_engine_snapshot() {
    let snapshot = EngineSnapshot::default();
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();
    harness.get_by_label("GROUPS");
    harness.get_by_label("INSPECTOR");
    harness.get_by_label("No Scenes yet");
}

/// The largest allowed difference between the vertical centres of the header
/// controls. One `ui.horizontal` row centres every widget, and the labels are a
/// point or two shorter than a button or a `DragValue`, so a few points of slack
/// keep the assertion about *rows* rather than exact font metrics.
const HEADER_LINE_SPREAD: f32 = 6.0;

/// Assert that every named widget's centre is on the same horizontal line.
fn assert_shared_header_line(harness: &Harness<'_, UiState>, labels: &[&str]) {
    let rects: Vec<(&str, egui::Rect)> = labels
        .iter()
        .map(|label| (*label, harness.get_by_label(label).rect()))
        .collect();
    let centres: Vec<f32> = rects.iter().map(|(_, rect)| rect.center().y).collect();
    let lowest = centres.iter().copied().fold(f32::INFINITY, f32::min);
    let highest = centres.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        highest - lowest <= HEADER_LINE_SPREAD,
        "the performance header must be one line: centres {centres:?} span {:.1} px \
         (max {HEADER_LINE_SPREAD}) for {rects:?}",
        highest - lowest
    );
}

#[test]
fn the_performance_header_controls_share_one_line() {
    let mut snapshot = EngineSnapshot::default();
    snapshot.performance.min_value = 8.0;
    snapshot.performance.max_value = 64.0;
    snapshot.performance.transition_seconds = 1.0;
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    assert_shared_header_line(
        &harness,
        &["Start performance", "min", "max", "fade", "Open output"],
    );

    // `Open output` stays pinned to the right and the controls must not run
    // underneath it even at the narrowest supported shell width.
    let open = harness.get_by_label("Open output").rect();
    for label in ["Start performance", "min", "max", "fade"] {
        let rect = harness.get_by_label(label).rect();
        assert!(
            rect.right() <= open.left(),
            "{label} at {rect:?} overlaps Open output at {open:?}"
        );
    }
}

#[test]
fn the_running_performance_header_keeps_the_chip_on_the_same_line() {
    let mut snapshot = EngineSnapshot::default();
    snapshot.performance.enabled = true;
    // The widest header: Random's label plus the bar-based hint with its
    // resolved second durations, which is what the running UI shows.
    snapshot.performance.order = "random".to_string();
    snapshot.performance.timing_unit = modv_ui::TimingUnit::Bars;
    snapshot.performance.min_value = 8.0;
    snapshot.performance.max_value = 64.0;
    snapshot.performance.transition_seconds = 1.0;
    snapshot.audio.bpm = 128.0;
    let hint = modv_ui::timing_hint(8.0, 64.0, modv_ui::TimingUnit::Bars, 128.0);
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    assert_shared_header_line(
        &harness,
        &[
            "Stop performance",
            "min",
            "max",
            "fade",
            "PERFORMANCE",
            "Open output",
        ],
    );
    assert!(
        harness.get_all_by_value(&hint).count() >= 1,
        "the resolved-duration hint stays visible in the running header: expected {hint:?}"
    );

    let open = harness.get_by_label("Open output").rect();
    let chip = harness.get_by_label("PERFORMANCE").rect();
    let hint_rect = harness
        .query_all_by_value(&hint)
        .next()
        .map(|node| node.rect())
        .expect("the hint has a rect");
    assert!(
        hint_rect.right() <= chip.left(),
        "the header must not run under the PERFORMANCE chip: hint {hint_rect:?}, chip {chip:?}"
    );
    assert!(
        chip.right() <= open.left(),
        "the chip stays left of Open output: chip {chip:?}, Open output {open:?}"
    );
}

#[test]
fn shell_scene_grid_renders_scene_columns_and_cells() {
    let mut snapshot = EngineSnapshot::default();
    snapshot.scenes.push(modv_ui::SceneView {
        id: "scene-a".to_string(),
        name: "Lone Chrome".to_string(),
        enabled: true,
        timing: Some((4.0, 8.0)),
        cells: vec![modv_ui::SceneCellView {
            row: 0,
            module_names: vec!["Bands".to_string()],
            disabled: false,
        }],
    });
    snapshot.scene_rows = vec!["Foreground".to_string()];
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();
    harness.get_by_label("Lone Chrome");
    harness.get_by_label("+ Column");
    harness.get_by_label("+ Row");
    harness.get_by_label("Row names");
}
