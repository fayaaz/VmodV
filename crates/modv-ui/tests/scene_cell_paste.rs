//! Copying a Scene-grid cell from the right-click menu must also allow pasting
//! it into a target cell from that target's right-click menu.
//!
//! The menu currently offers `Copy cell (Ctrl+drag)` but no Paste item, so a
//! copied cell is remembered in `SceneGridState::copy_source` yet can never be
//! written to another cell. This test copies from one cell, opens a second
//! cell's menu and asserts that a Paste item exists and emits the same
//! `scenes/copySceneCellToScene` action (with the same four payload keys) that
//! a Ctrl+drag drop emits.

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::panels::CellRef;
use modv_ui::{EngineSnapshot, PerformanceView, SceneCellView, SceneView, UiState};

fn scene(id: &str, name: &str) -> SceneView {
    SceneView {
        id: id.to_string(),
        name: name.to_string(),
        enabled: true,
        timing: None,
        cells: vec![
            SceneCellView {
                row: 0,
                module_names: vec!["Bands".to_string()],
                disabled: false,
            },
            SceneCellView {
                row: 1,
                module_names: vec!["Ball".to_string()],
                disabled: false,
            },
        ],
    }
}

fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        scenes: vec![scene("scene-a", "Alpha"), scene("scene-b", "Beta")],
        scene_rows: vec!["Background".to_string(), "Foreground".to_string()],
        // Performance mode is on, but without `active_scene_rows` the cell
        // labels stay plain (`Alpha row 1`), which keeps the test deterministic.
        performance: PerformanceView {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn harness() -> Harness<'static, UiState> {
    let mut harness = Harness::builder()
        .with_size([900.0, 480.0])
        .with_step_dt(1.0 / 60.0)
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
            UiState::new(),
        );
    modv_ui::theme::apply(&harness.ctx);
    harness.run();
    harness
}

fn secondary_click(harness: &mut Harness<'static, UiState>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.run();
    }
}

#[test]
fn copying_a_cell_from_the_menu_allows_pasting_it_into_another_cell() {
    let mut harness = harness();

    // Copy `scene-a` row 0 through the source cell's right-click menu.
    let source = harness.get_by_label("Alpha row 1").rect().center();
    secondary_click(&mut harness, source);
    assert!(
        harness.query_all_by_label("Copy cell (Ctrl+drag)").count() > 0,
        "the source cell's context menu must offer Copy cell"
    );
    harness.get_by_label("Copy cell (Ctrl+drag)").click();
    harness.run();
    harness.state_mut().take_actions();

    // Open a different cell's right-click menu and paste there.
    let target = harness.get_by_label("Beta row 2").rect().center();
    secondary_click(&mut harness, target);
    assert!(
        harness.query_all_by_label("Paste cell").count() > 0,
        "a cell context menu must offer 'Paste cell' after a cell was copied"
    );

    harness.get_by_label("Paste cell").click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    let copies: Vec<_> = actions
        .iter()
        .filter(|action| action.action == "scenes/copySceneCellToScene")
        .collect();
    assert_eq!(
        copies.len(),
        1,
        "pasting must queue exactly one cell-copy action: {actions:?}"
    );
    assert_eq!(
        copies[0].payload,
        serde_json::json!({
            "fromSceneId": "scene-a",
            "fromRowIndex": 0,
            "toSceneId": "scene-b",
            "toRowIndex": 1,
        }),
        "pasting must emit the same engine payload as dropping the copied cell"
    );
    assert_eq!(
        harness.state().scene_grid.copy_source,
        Some(CellRef::new("scene-a", 0)),
        "the copied cell is a durable clipboard source, not a one-shot drag"
    );
}
