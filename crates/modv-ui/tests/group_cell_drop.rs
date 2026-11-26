//! Group → Scene-cell drag and drop in the VmodV shell (BUGS.md item 11, UI).
//!
//! Dragging a Group header onto a Scene-grid cell uses the same
//! `scenes/setSceneCellGroup` action as the engine path. An occupied cell must
//! ask for confirmation first (the drag alone queues nothing, Cancel queues
//! nothing, Confirm queues the action); an empty cell queues the action
//! immediately with no dialog. These tests drive real pointer gestures through
//! the full shell so the missing drag source / drop target / confirmation is a
//! missing-behaviour failure rather than a reference to a not-yet-existing
//! payload type.

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, PerformanceView, SceneCellView, SceneView, UiState};

fn group(id: &str, name: &str) -> GroupView {
    GroupView {
        id: id.to_string(),
        name: name.to_string(),
        enabled: GROUP_ENABLED,
        hidden: false,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        inherit: true,
        inherit_from: InheritTarget::Previous,
        clearing: true,
        pipeline: false,
        module_ids: Vec::new(),
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
        groups: vec![group("group-a", "Background")],
        scenes: vec![SceneView {
            id: "scene-a".to_string(),
            name: "Alpha".to_string(),
            enabled: true,
            timing: None,
            // Row 0 is occupied (needs confirmation); row 1 is empty (no dialog).
            cells: vec![populated(0, &["Ball", "Bands"]), empty(1)],
        }],
        scene_rows: vec!["Row A".to_string(), "Row B".to_string()],
        performance: PerformanceView {
            // Keep the accessible cell label plain: "Alpha Group 1" / "Alpha Group 2".
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn harness() -> Harness<'static, UiState> {
    let snapshot = snapshot();
    Harness::builder()
        .with_size([1280.0, 900.0])
        .with_step_dt(1.0 / 60.0)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            UiState::new(),
        )
}

/// Drag from `from` to `to`, releasing with a raw pointer event. Same gesture
/// the gallery drag tests use: `drop_at` can clear the interact position before
/// the release is handled, so the release is sent as an explicit event.
fn drag(harness: &mut Harness<'static, UiState>, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

/// The Group header is the leftmost widget labelled "Background" (the Groups
/// pane); the Inspector may show the same name on the right.
fn group_header(harness: &Harness<'static, UiState>) -> egui::Pos2 {
    harness
        .get_all_by_label("Background")
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the Groups pane shows the Background Group")
        .rect()
        .center()
}

fn overwrite_actions(actions: &[modv_ui::UiAction]) -> Vec<&modv_ui::UiAction> {
    actions
        .iter()
        .filter(|action| action.action == "scenes/setSceneCellGroup")
        .collect()
}

#[test]
fn dragging_a_group_onto_an_occupied_cell_asks_before_overwriting() {
    let mut harness = harness();
    harness.run();

    let from = group_header(&harness);
    let to = harness.get_by_label("Alpha Group 1").rect().center();
    drag(&mut harness, from, to);

    // The drop alone must not overwrite, but must surface a confirmation with
    // an "Overwrite cell" button.
    let actions = harness.state_mut().take_actions();
    assert!(
        overwrite_actions(&actions).is_empty(),
        "no overwrite before confirmation: {actions:?}"
    );
    assert!(
        harness.query_all_by_label("Overwrite cell").count() > 0,
        "an overwrite confirmation with an 'Overwrite cell' button should be visible"
    );

    // Cancel closes the dialog and still queues nothing.
    harness.get_by_label("Cancel").click();
    harness.run();
    let actions = harness.state_mut().take_actions();
    assert!(
        overwrite_actions(&actions).is_empty(),
        "Cancel must not overwrite the cell: {actions:?}"
    );
}

#[test]
fn confirming_overwrites_the_cell() {
    let mut harness = harness();
    harness.run();

    let from = group_header(&harness);
    let to = harness.get_by_label("Alpha Group 1").rect().center();
    drag(&mut harness, from, to);

    assert!(
        harness.query_all_by_label("Overwrite cell").count() > 0,
        "the confirmation button should be visible before clicking it"
    );
    harness.get_by_label("Overwrite cell").click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    let overwrites = overwrite_actions(&actions);
    assert_eq!(overwrites.len(), 1, "one overwrite action: {actions:?}");
    assert_eq!(
        overwrites[0].payload,
        serde_json::json!({ "sceneId": "scene-a", "rowIndex": 0, "groupId": "group-a" })
    );
}

#[test]
fn dragging_a_group_onto_an_empty_cell_overwrites_without_confirmation() {
    let mut harness = harness();
    harness.run();

    let from = group_header(&harness);
    let to = harness.get_by_label("Alpha Group 2").rect().center();
    drag(&mut harness, from, to);

    assert_eq!(
        harness.query_all_by_label("Overwrite cell").count(),
        0,
        "an empty cell must not open a confirmation dialog"
    );

    let actions = harness.state_mut().take_actions();
    let overwrites = overwrite_actions(&actions);
    assert_eq!(
        overwrites.len(),
        1,
        "one immediate overwrite action: {actions:?}"
    );
    assert_eq!(
        overwrites[0].payload,
        serde_json::json!({ "sceneId": "scene-a", "rowIndex": 1, "groupId": "group-a" })
    );
}
