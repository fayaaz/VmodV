//! Port of the `tests/e2e/spec/scenes/performanceGrid.spec.js` Scenes-grid
//! intents that the VmodV grid implements.
//!
//! The VmodV grid has no dialog-based cell editor: it exposes the same
//! operations through the cell/header context menus and a double-click on an
//! empty cell. These tests drive those widgets and assert the exact
//! `scenes/*` payload. The engine half of each operation (create/clear/capture/
//! rename) is covered by the `modv-app` scene tests; the mutable fixture below
//! applies the queued action the same way so the grid re-render is asserted
//! too.
//!
//! ## Cell-editor waiver
//!
//! Two JS intents are deliberately waived because VmodV has no in-cell module
//! editor or module drag into that editor:
//!
//! - `editing a grid cell saves added modules back into the Scene` — the JS
//!   dialog opens `Edit`, adds modules, and saves them back into the Scene.
//!   VmodV provides the same end state without the dialog: double-click an
//!   empty cell to queue `scenes/createSceneCellGroup` (which stamps the row's
//!   Group into the Scene), edit the live Group (Gallery/drag into the Groups
//!   pane), then `Capture live row into this cell`
//!   (`scenes/captureSceneRowToScene`) from the cell context menu, and
//!   `Clear cell` (`scenes/clearSceneCell`, now confirmed) to empty it. Those
//!   actions are asserted below and their store halves live in
//!   `modv-app`'s `scene_ops` tests.
//! - `the cell editor can drag a module from its panel into the Group` — there
//!   is no editor module panel to drag from. VmodV's equivalent drag exists
//!   already: a Gallery tile dropped on a Group block queues
//!   `ui/addModuleToGroup` (`tests/gallery_drag_drop.rs`), and a Library asset
//!   can be dragged onto its texture prop.

use std::sync::{Arc, Mutex};

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use modv_ui::panels::CellRef;
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
            // `+ Column` sends `{}` and the engine mints `Scene N`.
            let name = action.payload["name"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("Scene {}", snapshot.scenes.len() + 1));
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
// `performanceGrid.spec.js`: "hovering a grid cell previews the saved group
// and leaves a still".
fn hovering_a_cell_previews_the_saved_group_and_keeps_the_still() {
    let (mut harness, snapshot) = build();
    let first_cell = CellRef::new("scene-a", 0);
    let second_cell = CellRef::new("scene-a", 1);

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
    assert!(
        harness
            .state()
            .scene_grid
            .paints_preview(&first_cell, false),
        "the first cell keeps painting its cached preview after the pointer moves"
    );

    // Hovering another cell asks for that row's preview. Both cells keep
    // painting: the legacy grid leaves a still per rendered cell, not just
    // the most recently hovered one.
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
    assert!(
        harness
            .state()
            .scene_grid
            .paints_preview(&first_cell, false),
        "the first cell's still survives hovering the second cell"
    );

    // Leaving the grid stops new requests but keeps both previews painted.
    harness.remove_cursor();
    harness.run();
    let requests = std::mem::take(&mut harness.state_mut().scene_grid.preview_requests);
    assert!(
        requests.is_empty(),
        "no preview is requested when the pointer leaves the grid: {requests:?}"
    );
    assert!(
        harness
            .state()
            .scene_grid
            .paints_preview(&first_cell, false),
        "the first row keeps its still"
    );
    assert!(
        harness
            .state()
            .scene_grid
            .paints_preview(&second_cell, false),
        "the second row keeps its still"
    );

    // The still paints from the snapshot's cached texture.
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

    // Editing the saved cell drops its still; the cached frame no longer
    // matches what the Scene holds.
    snapshot.lock().unwrap().scenes[0].cells[1].module_names = vec!["Ball".to_string()];
    harness.run();
    assert!(
        !harness
            .state()
            .scene_grid
            .paints_preview(&second_cell, false),
        "a changed cell stops painting the stale still"
    );
    assert!(
        harness
            .state()
            .scene_grid
            .paints_preview(&first_cell, false),
        "an unchanged cell keeps its still"
    );

    // Clearing the cell drops its still; the Scene no longer has a preview to
    // show.
    snapshot.lock().unwrap().scenes[0].cells[0]
        .module_names
        .clear();
    harness.run();
    assert!(
        !harness
            .state()
            .scene_grid
            .paints_preview(&first_cell, false),
        "an emptied cell stops painting the stale still"
    );
}

#[test]
// `performanceGrid.spec.js`: "the Scenes grid adds a new empty Scene column".
fn the_grid_adds_a_new_empty_scene_column() {
    let (mut harness, snapshot) = build();
    assert_eq!(snapshot.lock().unwrap().scenes.len(), 1);

    // The row/column controls sit directly above the Scene cells.
    let controls_bottom = harness.get_by_label("+ Column").rect().bottom();
    let header_top = harness.get_by_label("Alpha").rect().top();
    assert!(
        controls_bottom <= header_top,
        "the grid controls sit above the cells: {controls_bottom} <= {header_top}"
    );

    harness.get_by_label("+ Column").click();
    let actions = run_and_apply(&mut harness, &snapshot);

    let adds = find(&actions, "scenes/addEmptyScene");
    assert_eq!(adds.len(), 1, "one add-Scene action: {actions:?}");
    assert_eq!(
        adds[0].payload,
        serde_json::json!({}),
        "the engine mints the `Scene N` name; the UI does not supply one"
    );

    let scenes = snapshot.lock().unwrap();
    assert_eq!(scenes.scenes.len(), 2);
    let added = &scenes.scenes[1];
    assert!(
        added.name.starts_with("Scene "),
        "the new column uses the engine's `Scene N` default: {}",
        added.name
    );
    assert!(added.cells.is_empty(), "a new column has no cells");
    let name = added.name.clone();
    drop(scenes);
    assert!(
        harness.get_all_by_label(&name).count() >= 1,
        "the new column header appears in the grid"
    );
}

#[test]
// `performanceGrid.spec.js`: "clearing a grid cell removes its modules after
// confirmation".
fn clearing_a_cell_asks_for_confirmation_first() {
    let (mut harness, snapshot) = build();

    let cell = harness.get_by_label("Alpha row 1").rect();
    secondary_click(&mut harness, cell.center());
    harness.get_by_label("Clear cell").click();
    harness.run();

    // The menu item only opens the confirmation; nothing is cleared yet.
    let actions = harness.state_mut().take_actions();
    assert!(
        find(&actions, "scenes/clearSceneCell").is_empty(),
        "the menu item does not clear the cell directly: {actions:?}"
    );
    assert!(
        harness.state().scene_grid.clear_cell_edit.is_some(),
        "the confirmation dialog is open"
    );
    assert_eq!(
        snapshot.lock().unwrap().scenes[0].cells[0].module_names,
        ["Ball", "Bands"],
        "Cancel is available before anything is written"
    );

    // Cancel closes the dialog and leaves the saved cell alone.
    harness.get_by_label("Cancel").click();
    let actions = run_and_apply(&mut harness, &snapshot);
    assert!(
        find(&actions, "scenes/clearSceneCell").is_empty(),
        "Cancel does nothing: {actions:?}"
    );
    assert!(harness.state().scene_grid.clear_cell_edit.is_none());
    assert_eq!(
        harness.get_by_label("Alpha row 1").accesskit_node().label(),
        Some("Alpha row 1".to_string()),
        "the cell keeps its modules"
    );

    // Confirming queues the same payload the old direct item sent.
    secondary_click(&mut harness, cell.center());
    harness.get_by_label("Clear cell").click();
    harness.run();
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
// `performanceGrid.spec.js`: "editing a grid cell saves added modules back
// into the Scene" (VmodV live-capture path).
fn capturing_the_live_row_saves_the_added_modules_into_the_cell() {
    // Waived cell-editor intent: "editing a grid cell saves added modules
    // back into the Scene". VmodV has no in-cell editor dialog; its
    // equivalent "edit this cell" path is: add a module to the live Group
    // (Gallery), then capture that live row back into the saved cell. The
    // store half is covered by
    // scene_ops.rs:scene_cell_actions_create_capture_and_clear_rows.
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
// `performanceGrid.spec.js`: "renaming a Scene column updates the grid and
// the Scene".
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
// `performanceGrid.spec.js`: "editing an empty Scene cell creates a Group and
// saves its modules" (the create half).
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
