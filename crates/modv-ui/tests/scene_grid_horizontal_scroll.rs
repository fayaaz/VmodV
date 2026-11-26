//! Regression: the Scenes grid cannot be scrolled sideways from its resting
//! position.
//!
//! The grid puts the Scene columns in `ScrollArea::both()` and paints the layer
//! gutter as a child UI over the left edge of the viewport. That gutter's row
//! widgets extend over the left end of the horizontal scrollbar's interaction
//! strip. At scroll offset zero the scrollbar thumb is at that left end, so
//! grabbing it does nothing: the pointer lands on a gutter row instead of the
//! scrollbar and the Scene columns stay put even though they overflow the
//! viewport. Scrolling sideways with the wheel works, which is why the existing
//! `scene_grid_layout.rs` wheel test does not catch this.
//!
//! This test grabs the horizontal scrollbar exactly where it sits at rest and
//! asserts that the Scene columns scroll sideways while the gutter stays
//! pinned and its rows stay aligned.

use egui::Pos2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, PerformanceView, SceneCellView, SceneView, UiState};

const ROW_NAMES: [&str; 5] = ["WEBCAM", "Background", "Foreground", "Effects", "Master"];
const SCENES: [(&str, &str); 4] = [
    ("scene-alpha", "Alpha"),
    ("scene-beta", "Beta"),
    ("scene-gamma", "Gamma"),
    ("scene-delta", "Delta"),
];

fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        scenes: SCENES
            .iter()
            .map(|(id, name)| SceneView {
                id: (*id).to_string(),
                name: (*name).to_string(),
                enabled: true,
                timing: None,
                cells: (0..5)
                    .map(|row| SceneCellView {
                        row,
                        module_names: Vec::new(),
                        disabled: false,
                    })
                    .collect(),
            })
            .collect(),
        scene_rows: ROW_NAMES.iter().map(|name| (*name).to_string()).collect(),
        performance: PerformanceView {
            enabled: true,
            current_scene_id: Some("scene-alpha".to_string()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn harness() -> Harness<'static, UiState> {
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size([640.0, 420.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let snapshot = snapshot();
                modv_ui::panels::draw_scene_grid(
                    ui,
                    &snapshot,
                    &mut state.scene_grid,
                    None,
                    &mut state.actions,
                    &mut state.layout,
                );
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness.run();
    harness
}

fn cell_rect(harness: &Harness<'_, UiState>, scene: &str, row: usize) -> egui::Rect {
    harness.get_by_label(&format!("{scene} Group {row}")).rect()
}

fn layer_rect(harness: &Harness<'_, UiState>, row: usize) -> egui::Rect {
    harness
        .get_by_label(&format!(
            "Group {row}: {}",
            ROW_NAMES[row.saturating_sub(1)]
        ))
        .rect()
}

/// Drag the pointer from `from` to `to` in small steps so egui sees a real drag
/// gesture (the scrollbar needs a press followed by pointer moves).
fn drag(harness: &mut Harness<'static, UiState>, from: Pos2, to: Pos2) {
    harness.hover_at(from);
    harness.run_steps(4);
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(4);
    for step in 1..=10 {
        let t = step as f32 / 10.0;
        harness.event(egui::Event::PointerMoved(Pos2::new(
            from.x + (to.x - from.x) * t,
            from.y + (to.y - from.y) * t,
        )));
        harness.run_steps(2);
    }
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(4);
}

#[test]
fn dragging_the_horizontal_scrollbar_at_rest_scrolls_the_scene_columns() {
    let mut harness = harness();

    let cell_before = cell_rect(&harness, "Alpha", 2);
    let gutter_before = layer_rect(&harness, 2);

    // With `VisibleWhenNeeded`, the horizontal bar only exists when the Scene
    // columns overflow this narrow viewport.
    let bars: Vec<egui::Rect> = harness
        .get_all_by_role(egui::Role::ScrollBar)
        .map(|node| node.rect())
        .collect();
    let bar = *bars
        .iter()
        .find(|rect| rect.width() > rect.height())
        .expect("a horizontal scrollbar: the Scene columns must overflow the viewport");

    // At scroll offset zero the thumb is at the far left of the track. Grab it
    // there (inside the pinned gutter's x range) and pull it right.
    let from = Pos2::new(bar.min.x + 40.0, bar.center().y);
    let to = Pos2::new(from.x + 160.0, from.y);
    drag(&mut harness, from, to);

    let cell_after = cell_rect(&harness, "Alpha", 2);
    let gutter_after = layer_rect(&harness, 2);

    assert!(
        cell_after.left() < cell_before.left() - 10.0,
        "dragging the horizontal scrollbar at its resting position must scroll the Scene columns sideways: {cell_before:?} -> {cell_after:?} (bar {bar:?}, drag {from:?} -> {to:?})"
    );
    assert!(
        (gutter_after.left() - gutter_before.left()).abs() <= 0.1,
        "the layer gutter stays pinned to the viewport's left edge: {gutter_before:?} -> {gutter_after:?}"
    );
    assert!(
        (cell_after.top() - gutter_after.top()).abs() <= 0.1,
        "the pinned gutter row stays aligned with its Scene row: cell {cell_after:?}, gutter {gutter_after:?}"
    );
}
