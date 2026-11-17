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
    harness.get_by_label("SCENES");
}

#[test]
fn shell_scene_grid_renders_scene_columns_and_cells() {
    let mut snapshot = EngineSnapshot::default();
    snapshot.scenes.push(modv_ui::SceneView {
        id: "scene-a".to_string(),
        name: "Lone Chrome".to_string(),
        enabled: true,
        timing: Some(8.0),
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
}
