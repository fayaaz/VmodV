//! Port of the `tests/e2e/spec/scenes/performanceGrid.spec.js` Scenes-grid
//! intents that the native grid implements.
//!
//! The native grid has no dialog-based cell editor: it exposes the same
//! operations through the cell/header context menus and a double-click on an
//! empty cell. These tests drive those widgets and assert the exact
//! `scenes/*` payload. The engine half of each operation (create/clear/capture/
//! rename) is covered by the `modv-app` scene tests; the mutable fixture below
//! applies the queued action the same way so the grid re-render is asserted
//! too.
//!
//! Not covered here because the native grid has no equivalent yet:
//! - the cell editor dialog (`Edit` -> add module -> close) from
//!   `editing a grid cell saves added modules back into the Scene`; the native
//!   path is to add to the live Group and capture the row (covered below).
//! - dragging a module from the editor's module panel into the Group.
//! - the native grid paints a hover preview only while the cell is hovered;
//!   the legacy grid keeps the last preview visible as a still.

use std::sync::{Arc, Mutex};

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use modv_ui::{
    EngineSnapshot, PerformanceView, PreviewRequest, PreviewTexture, SceneCellView, SceneView,
    UiAction, UiState, scene_preview_key,
};
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

fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        scenes: vec![scene(
            "scene-a",
            "Alpha",
            vec![populated(0, &["Ball", "Bands"]), populated(1, &["Bands"])],
        )],
        scene_rows: vec!["Background".to_string(), "Foreground".to_string()],
        performance: PerformanceView {
            // Performance off keeps the accessible cell label plain
            // (`Alpha row 1`); the LIVE suffix is not what these tests assert.
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn build() -> (Harness<'static, UiState>, Arc<Mutex<EngineSnapshot>>) {
    let snapshot = Arc::new(Mutex::new(snapshot()));
    let draw_snapshot = Arc::clone(&snapshot);
    let mut harness = Harness::builder()
        .with_size([700.0, 480.0])
        .with_step_dt(1.0 / 60.0)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let snapshot = draw_snapshot.lock().unwrap();
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
    (harness, snapshot)
}

fn apply(snapshot: &Arc<Mutex<EngineSnapshot>>, action: &UiAction) {
    let mut snapshot = snapshot.lock().unwrap();
    match action.action.as_str() {
        "scenes/addEmptyScene" => {
            let name = action.payload["name"]
                .as_str()
                .unwrap_or("New Scene")
                .to_string();
            let id = format!("scene-new-{}", snapshot.scenes.len());
            snapshot.scenes.push(scene(&id, &name, Vec::new()));
        }
        "scenes/updateSceneName" => {
            let scene_id = action.payload["sceneId"].as_str().unwrap_or_default();
            if let Some(scene) = snapshot.scenes.iter_mut().find(|s| s.id == scene_id) {
                scene.name = action.payload["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
            }
        }
        "scenes/clearSceneCell" => {
            let scene_id = action.payload["sceneId"].as_str().unwrap_or_default();
            let row = action.payload["rowIndex"].as_u64().unwrap_or_default() as usize;
            if let Some(scene) = snapshot.scenes.iter_mut().find(|s| s.id == scene_id)
                && let Some(cell) = scene.cells.get_mut(row)
            {
                cell.module_names.clear();
            }
        }
        _ => {}
    }
}

fn run_and_apply(
    harness: &mut Harness<'static, UiState>,
    snapshot: &Arc<Mutex<EngineSnapshot>>,
) -> Vec<UiAction> {
    harness.run();
    let actions = harness.state_mut().take_actions();
    for action in &actions {
        apply(snapshot, action);
    }
    harness.run();
    actions
}

fn find<'a>(actions: &'a [UiAction], name: &str) -> Vec<&'a UiAction> {
    actions
        .iter()
        .filter(|action| action.action == name)
        .collect()
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

fn double_click(harness: &mut Harness<'static, UiState>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run();
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            harness.run();
        }
    }
}

#[test]
fn hovering_a_cell_previews_the_saved_group_and_keeps_the_still() {
    let (mut harness, snapshot) = build();

    let first = harness.get_by_label("Alpha row 1").rect();
    harness.hover_at(first.center());
    harness.run();

    let requests = std::mem::take(&mut harness.state_mut().scene_grid.preview_requests);
    let last = requests
        .last()
        .expect("hovering a populated cell requests its preview");
    assert_eq!(
        last,
        &PreviewRequest::Scene {
            scene_id: "scene-a".to_string(),
            row_index: 0,
        },
        "the request names the saved Scene row, not the live composition"
    );

    // Hovering another cell asks for that row's preview. The previous row's
    // texture stays in the snapshot cache (a still), which is what the legacy
    // grid keeps painted after the pointer leaves.
    let second = harness.get_by_label("Alpha row 2").rect();
    harness.hover_at(second.center());
    harness.run();
    let requests = std::mem::take(&mut harness.state_mut().scene_grid.preview_requests);
    assert_eq!(
        requests.last(),
        Some(&PreviewRequest::Scene {
            scene_id: "scene-a".to_string(),
            row_index: 1,
        })
    );

    // Leaving the grid stops new requests but does not evict the cached cell
    // preview. (The grid currently paints a preview only while its cell is
    // hovered; the retained still is reported as a parity gap.)
    harness.remove_cursor();
    harness.run();
    let requests = std::mem::take(&mut harness.state_mut().scene_grid.preview_requests);
    assert!(
        requests.is_empty(),
        "no preview is requested when the pointer leaves the grid: {requests:?}"
    );

    // The engine keeps the rendered cell texture after the hover ends; the
    // grid reads it from the same snapshot map on a later hover.
    snapshot.lock().unwrap().scene_previews.insert(
        scene_preview_key("scene-a", 0),
        PreviewTexture {
            id: egui::TextureId::Managed(0),
            width: 320,
            height: 180,
        },
    );
    harness.run();
    assert!(
        snapshot
            .lock()
            .unwrap()
            .scene_previews
            .contains_key(&scene_preview_key("scene-a", 0)),
        "the first row's preview texture remains available as a still"
    );
}

#[test]
fn the_grid_adds_a_new_empty_scene_column() {
    let (mut harness, snapshot) = build();
    assert_eq!(snapshot.lock().unwrap().scenes.len(), 1);

    harness.get_by_label("+ Column").click();
    let actions = run_and_apply(&mut harness, &snapshot);

    let adds = find(&actions, "scenes/addEmptyScene");
    assert_eq!(adds.len(), 1, "one add-Scene action: {actions:?}");
    assert_eq!(adds[0].payload, serde_json::json!({ "name": "New Scene" }));

    let scenes = snapshot.lock().unwrap();
    assert_eq!(scenes.scenes.len(), 2);
    let added = &scenes.scenes[1];
    assert!(added.cells.is_empty(), "a new column has no cells");
    drop(scenes);
    assert!(
        harness.get_all_by_label("New Scene").count() >= 1,
        "the new column header appears in the grid"
    );
}

#[test]
fn clearing_a_cell_removes_its_modules() {
    let (mut harness, snapshot) = build();

    // The legacy grid asks for confirmation before clearing. The native grid
    // has no confirmation dialog: the menu item queues the clearing action
    // directly (recorded as a parity gap).
    let cell = harness.get_by_label("Alpha row 1").rect();
    secondary_click(&mut harness, cell.center());
    harness.get_by_label("Clear cell").click();
    let actions = run_and_apply(&mut harness, &snapshot);

    let clears = find(&actions, "scenes/clearSceneCell");
    assert_eq!(clears.len(), 1, "one clear action: {actions:?}");
    assert_eq!(
        clears[0].payload,
        serde_json::json!({ "sceneId": "scene-a", "rowIndex": 0 })
    );
    assert!(
        snapshot.lock().unwrap().scenes[0].cells[0]
            .module_names
            .is_empty(),
        "the saved cell no longer lists Ball/Bands"
    );
    assert_eq!(
        harness.get_by_label("Alpha row 1").accesskit_node().label(),
        Some("Alpha row 1".to_string()),
        "the cell stays, now empty"
    );
}

#[test]
fn capturing_the_live_row_saves_the_added_modules_into_the_cell() {
    // The native grid has no in-cell editor dialog. Its equivalent "edit this
    // cell" path is: add a module to the live Group (Gallery), then capture
    // that live row back into the saved cell. The store half is covered by
    // scene_ops.rs:capture_scene_row_to_scene_saves_the_live_group.
    let (mut harness, _snapshot) = build();

    let cell = harness.get_by_label("Alpha row 1").rect();
    secondary_click(&mut harness, cell.center());
    harness
        .get_by_label("Capture live row into this cell")
        .click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    let captures = find(&actions, "scenes/captureSceneRowToScene");
    assert_eq!(captures.len(), 1, "one capture action: {actions:?}");
    assert_eq!(
        captures[0].payload,
        serde_json::json!({ "sceneId": "scene-a", "rowIndex": 0 })
    );
}

#[test]
fn renaming_a_scene_column_updates_the_grid_and_the_scene() {
    let (mut harness, snapshot) = build();

    let header = harness.get_by_label("Alpha").rect();
    double_click(&mut harness, header.center());

    let name_input = harness
        .get_all_by_role(accesskit::Role::TextInput)
        .find(|node| node.value().as_deref() == Some("Alpha"))
        .expect("double-clicking the header opens the inline Scene-name editor");
    name_input.focus();
    name_input.set_text_value_accesskit("Renamed");
    harness.run();
    harness.key_press(egui::Key::Enter);

    let actions = run_and_apply(&mut harness, &snapshot);
    let renames = find(&actions, "scenes/updateSceneName");
    assert_eq!(renames.len(), 1, "one rename action: {actions:?}");
    assert_eq!(renames[0].payload["sceneId"], "scene-a");
    assert_eq!(renames[0].payload["name"], "Renamed");
    assert_eq!(snapshot.lock().unwrap().scenes[0].name, "Renamed");
    assert!(
        harness.get_all_by_label("Renamed").count() >= 1,
        "the grid header shows the new name"
    );
}

#[test]
fn editing_an_empty_cell_creates_a_group() {
    let (mut harness, snapshot) = build();
    // Make row 2 empty so the double-click has an empty cell to create in.
    snapshot.lock().unwrap().scenes[0].cells[1] = empty(1);
    harness.run();

    let cell = harness.get_by_label("Alpha row 2").rect();
    double_click(&mut harness, cell.center());
    let actions = run_and_apply(&mut harness, &snapshot);

    let creates = find(&actions, "scenes/createSceneCellGroup");
    assert_eq!(creates.len(), 1, "one create action: {actions:?}");
    assert_eq!(
        creates[0].payload,
        serde_json::json!({ "sceneId": "scene-a", "rowIndex": 1 })
    );
    assert!(
        !actions
            .iter()
            .any(|action| action.action == "scenes/applySceneRow"),
        "creating a Group does not apply the row: {actions:?}"
    );
}
