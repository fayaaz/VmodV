//! Gallery → Group drag and drop in the native shell.
//!
//! Electron's Gallery has been a drag source for a long time; its Group module
//! strips are drop targets. The native shell has the double-click and context
//! menu paths, so these tests pin the missing pointer path: dragging a tile
//! onto a Group queues the same `ui/addModuleToGroup` action for that Group,
//! and releasing over empty space queues nothing.

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, ModuleMetaView, ModuleView, UiState};

fn meta(name: &str) -> ModuleMetaView {
    ModuleMetaView {
        name: name.to_string(),
        module_type: "shader".to_string(),
        category: "effects".to_string(),
        author: "modV".to_string(),
        version: "1.0".to_string(),
        description: String::new(),
    }
}

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

fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        groups: vec![group("group-a", "Background"), group("group-b", "Group B")],
        registered: vec![meta("Bands"), meta("Ball")],
        ..Default::default()
    }
}

/// A Group that already owns a `Bands` module, so the module strip has a row
/// to drop onto (and the duplicate case is exercised).
fn snapshot_with_module() -> EngineSnapshot {
    let mut groups = vec![group("group-a", "Background"), group("group-b", "Group B")];
    groups[0].module_ids.push("bands-1".to_string());
    EngineSnapshot {
        groups,
        modules: vec![ModuleView {
            id: "bands-1".to_string(),
            module_name: "Bands".to_string(),
            group_id: "group-a".to_string(),
            enabled: true,
            alpha: 1.0,
            alpha_input_id: None,
            composite_operation: "normal".to_string(),
            composite_operation_input_id: None,
            module_type: "shader".to_string(),
            props: Vec::new(),
        }],
        registered: vec![meta("Bands"), meta("Ball")],
        ..Default::default()
    }
}

fn harness_for(
    snapshot: EngineSnapshot,
    selected_group: Option<&str>,
) -> Harness<'static, UiState> {
    let mut state = UiState::new();
    state.selected_group = selected_group.map(str::to_string);
    Harness::builder()
        .with_size([1280.0, 800.0])
        .with_step_dt(1.0 / 60.0)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        )
}

fn harness(selected_group: Option<&str>) -> Harness<'static, UiState> {
    harness_for(snapshot(), selected_group)
}

/// Drag from `from` to `to`, releasing with a raw pointer event. `drop_at`
/// also emits `PointerGone`, which can clear the interact position before the
/// release is handled (the same caveat the tween editor test documents).
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

#[test]
fn dragging_a_gallery_tile_onto_a_group_queues_the_add_action() {
    let mut harness = harness(None);
    harness.run();

    let tile = harness.get_by_label("Bands").rect().center();
    let target = harness
        .get_all_by_label("Background")
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the Groups pane shows the Background Group")
        .rect()
        .center();
    drag(&mut harness, tile, target);

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "one action queued: {actions:?}");
    assert_eq!(actions[0].action, "ui/addModuleToGroup");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({ "moduleName": "Bands", "groupId": "group-a" })
    );
}

#[test]
fn dragging_onto_a_different_group_targets_that_group() {
    let mut harness = harness(Some("group-a"));
    harness.run();

    let tile = harness.get_by_label("Ball").rect().center();
    let target = harness.get_by_label("Group B").rect().center();
    drag(&mut harness, tile, target);

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "one action queued: {actions:?}");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({ "moduleName": "Ball", "groupId": "group-b" }),
        "the hovered Group wins over the selected one"
    );
}

#[test]
fn dropping_on_a_groups_module_strip_queues_the_add_action() {
    let mut harness = harness_for(snapshot_with_module(), None);
    harness.run();

    // The row under "Background" belongs to Group A's module strip; the Group
    // already has Bands, and the drop still queues the same action so the
    // engine decides what a duplicate means.
    let strip = harness.get_by_label("Bands  (bands-1)").rect().center();
    let tile = harness.get_by_label("Bands").rect().center();
    drag(&mut harness, tile, strip);

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "one action queued: {actions:?}");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({ "moduleName": "Bands", "groupId": "group-a" })
    );
}

#[test]
fn releasing_a_gallery_drag_over_empty_space_queues_nothing() {
    let mut harness = harness(None);
    harness.run();

    let tile = harness.get_by_label("Bands").rect().center();
    let empty_space = harness.get_by_label("GALLERY").rect().center();
    drag(&mut harness, tile, empty_space);

    assert!(
        harness.state().actions.is_empty(),
        "released outside a Group: {:?}",
        harness.state().actions
    );
}

#[test]
fn double_clicking_a_gallery_tile_still_adds_to_the_selected_group() {
    let mut harness = harness(Some("group-a"));
    harness.run();

    let tile = harness.get_by_label("Bands");
    tile.click();
    tile.click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "one action queued: {actions:?}");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({ "moduleName": "Bands", "groupId": "group-a" })
    );
    assert_eq!(
        harness.state().selected_gallery_module.as_deref(),
        Some("Bands")
    );
}
