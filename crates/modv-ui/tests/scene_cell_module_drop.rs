//! BUGS.md #10 (UI half): a module dragged from the Gallery must be droppable
//! on ANY Scenes-grid cell — populated or empty. On release the grid queues
//! `scenes/addModuleToCell { sceneId, rowIndex, moduleName }` and consumes the
//! drag payload so another drop target cannot answer the same release.
//!
//! These tests drive `draw_scene_grid` directly (the same harness shape as
//! `e2e_scenes_grid.rs`) and inject a [`GalleryDrag`] payload straight into
//! egui's drag-and-drop state, then emit a raw primary-button *release* over a
//! cell. A release without a preceding press exercises only the drop path, so
//! the cell's click/double-click actions cannot be mistaken for a module drop.

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::gallery_drag::GalleryDrag;
use modv_ui::{EngineSnapshot, PerformanceView, SceneCellView, SceneView, UiState};

fn scene(id: &str, name: &str, cells: Vec<SceneCellView>) -> SceneView {
    SceneView {
        id: id.to_string(),
        name: name.to_string(),
        enabled: true,
        timing: None,
        cells,
    }
}

fn populated(row: usize, names: &[&str]) -> SceneCellView {
    SceneCellView {
        row,
        module_names: names.iter().map(|name| (*name).to_string()).collect(),
        disabled: false,
    }
}

fn empty(row: usize) -> SceneCellView {
    SceneCellView {
        row,
        module_names: Vec::new(),
        disabled: false,
    }
}

/// One Scene with a populated cell at row 0 and an EMPTY cell at row 1.
fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        scenes: vec![scene(
            "scene-a",
            "Alpha",
            vec![populated(0, &["Ball", "Bands"]), empty(1)],
        )],
        scene_rows: vec!["Background".to_string(), "Foreground".to_string()],
        performance: PerformanceView {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn build() -> Harness<'static, UiState> {
    let snapshot = snapshot();
    Harness::builder()
        .with_size([700.0, 480.0])
        .with_step_dt(1.0 / 60.0)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
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
        )
}

/// Inject a Gallery module payload, hover `target`, then release the primary
/// button there. The payload is set on the frame before the release so the
/// drop target sees both `has_payload_of_type` and `pointer.any_released()`.
fn drop_module_on(harness: &mut Harness<'static, UiState>, module: &str, target: egui::Pos2) {
    egui::DragAndDrop::set_payload(&harness.ctx, GalleryDrag::new(module));
    harness.hover_at(target);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: target,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

#[test]
fn a_gallery_module_dropped_on_a_populated_cell_queues_add_module_to_cell() {
    let mut harness = build();
    harness.run();

    let cell = harness.get_by_label("Alpha Group 1").rect().center();
    drop_module_on(&mut harness, "Bands", cell);

    let actions = harness.state_mut().take_actions();
    let adds: Vec<_> = actions
        .iter()
        .filter(|action| action.action == "scenes/addModuleToCell")
        .collect();
    assert_eq!(adds.len(), 1, "one add-module action: {actions:?}");
    assert_eq!(
        adds[0].payload,
        serde_json::json!({ "sceneId": "scene-a", "rowIndex": 0, "moduleName": "Bands" })
    );
    assert!(
        !egui::DragAndDrop::has_payload_of_type::<GalleryDrag>(&harness.ctx),
        "the released payload must be consumed, not left for another target"
    );
}

#[test]
fn a_gallery_module_dropped_on_an_empty_cell_queues_add_module_to_cell() {
    let mut harness = build();
    harness.run();

    // Row 2 has no modules (`empty(1)` in the fixture); the drag must still be
    // accepted — this is the reported bug.
    let cell = harness.get_by_label("Alpha Group 2").rect().center();
    drop_module_on(&mut harness, "Ball", cell);

    let actions = harness.state_mut().take_actions();
    let adds: Vec<_> = actions
        .iter()
        .filter(|action| action.action == "scenes/addModuleToCell")
        .collect();
    assert_eq!(adds.len(), 1, "one add-module action: {actions:?}");
    assert_eq!(
        adds[0].payload,
        serde_json::json!({ "sceneId": "scene-a", "rowIndex": 1, "moduleName": "Ball" })
    );
    assert!(
        !egui::DragAndDrop::has_payload_of_type::<GalleryDrag>(&harness.ctx),
        "the released payload must be consumed, not left for another target"
    );
}

#[test]
fn releasing_over_a_cell_without_a_module_payload_queues_nothing() {
    let mut harness = build();
    harness.run();

    let cell = harness.get_by_label("Alpha Group 1").rect().center();
    harness.hover_at(cell);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: cell,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert!(
        actions.is_empty(),
        "no GalleryDrag payload, no add-module action: {actions:?}"
    );
}
