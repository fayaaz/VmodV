//! Module drag and drop in the native Groups pane.
//!
//! Modules are re-ordered and moved between Groups by dragging their two-bar
//! grip: the upper half of a row inserts above it, the lower half below it, and
//! a Group's empty space appends after its last row. Every drop queues one
//! `groups/MOVE_MODULE`; releasing over empty space queues nothing. The module
//! name is click-to-select only, so dragging it starts no drag. The old per-row
//! `^`/`v` buttons are gone, so these tests also pin their absence.

use std::sync::{Arc, Mutex};

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, ModuleView, UiAction, UiState};

fn meta(name: &str) -> modv_ui::ModuleMetaView {
    modv_ui::ModuleMetaView {
        name: name.to_string(),
        module_type: "shader".to_string(),
        category: "effects".to_string(),
        author: "modV".to_string(),
        version: "1.0".to_string(),
        description: String::new(),
    }
}

fn group(id: &str, name: &str, module_ids: &[&str]) -> GroupView {
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
        clearing: false,
        pipeline: false,
        module_ids: module_ids.iter().map(|id| (*id).to_string()).collect(),
    }
}

fn module(id: &str, name: &str, group_id: &str) -> ModuleView {
    ModuleView {
        id: id.to_string(),
        module_name: name.to_string(),
        group_id: group_id.to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "shader".to_string(),
        props: Vec::new(),
    }
}

/// Two populated Groups plus an empty one: enough to cover same-Group
/// re-ordering, a cross-Group insert and an append into an empty Group.
fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        groups: vec![
            group("group-a", "Background", &["bands-1", "ball-1", "tunnel-1"]),
            group("group-b", "Group B", &["plasma-1"]),
            group("group-c", "Empty", &[]),
        ],
        modules: vec![
            module("bands-1", "Bands", "group-a"),
            module("ball-1", "Ball", "group-a"),
            module("tunnel-1", "Tunnel", "group-a"),
            module("plasma-1", "Plasma", "group-b"),
        ],
        registered: vec![meta("Bands"), meta("Ball"), meta("Tunnel"), meta("Plasma")],
        ..Default::default()
    }
}

fn build() -> (Harness<'static, UiState>, Arc<Mutex<EngineSnapshot>>) {
    let snapshot = Arc::new(Mutex::new(snapshot()));
    let draw_snapshot = Arc::clone(&snapshot);
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_step_dt(1.0 / 60.0)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let snapshot = draw_snapshot.lock().unwrap();
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            UiState::new(),
        );
    modv_ui::theme::apply(&harness.ctx);
    // The first pass seeds the startup Group focus and measures the panel;
    // run again so the accesskit rects kittest clicks match the settled
    // layout.
    harness.run();
    harness.run();
    (harness, snapshot)
}

/// Apply a queued `groups/MOVE_MODULE` to the fixture exactly like the engine
/// store: remove the id, then insert it at the clamped destination index.
fn apply_move(snapshot: &Arc<Mutex<EngineSnapshot>>, action: &UiAction) {
    assert_eq!(action.action, "groups/MOVE_MODULE", "unexpected action");
    let module_id = action.payload["moduleId"].as_str().expect("moduleId");
    let from_group_id = action.payload["fromGroupId"].as_str().expect("from");
    let to_group_id = action.payload["toGroupId"].as_str().expect("to");
    let to_index = action.payload["toIndex"].as_u64().expect("toIndex") as usize;

    let mut snapshot = snapshot.lock().unwrap();
    for group in &mut snapshot.groups {
        group.module_ids.retain(|id| id != module_id);
    }
    let Some(group) = snapshot
        .groups
        .iter_mut()
        .find(|group| group.id == to_group_id)
    else {
        panic!("no destination Group {to_group_id}");
    };
    let index = to_index.min(group.module_ids.len());
    group.module_ids.insert(index, module_id.to_string());
    for module in &mut snapshot.modules {
        if module.id == module_id {
            module.group_id = to_group_id.to_string();
        }
    }
    let _ = from_group_id;
}

/// Press on `from` and move the pointer to `to`, leaving the button down, so a
/// test can inspect the drag while it is still in flight.
fn drag_hold(harness: &mut Harness<'static, UiState>, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(to);
    harness.run();
}

/// Release the pointer at `to` with a raw event. `drop_at` also emits
/// `PointerGone`, which can clear the interact position before the release is
/// handled (the same caveat the tween editor test documents).
fn release_at(harness: &mut Harness<'static, UiState>, to: egui::Pos2) {
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

/// Drag from `from` to `to` and release there.
fn drag(harness: &mut Harness<'static, UiState>, from: egui::Pos2, to: egui::Pos2) {
    drag_hold(harness, from, to);
    release_at(harness, to);
}

fn module_row(harness: &Harness<'static, UiState>, label: &str) -> egui::Rect {
    harness.get_by_label(label).rect()
}

/// The centre of a module row's two-bar drag grip, addressed by its accessible
/// label.
fn module_grip(harness: &Harness<'static, UiState>, label: &str) -> egui::Pos2 {
    harness.get_by_label(label).rect().center()
}

#[test]
fn dragging_a_module_below_a_sibling_moves_it_down_within_the_group() {
    let (mut harness, snapshot) = build();
    let bands = module_grip(&harness, "Drag Bands (bands-1)");
    let ball = module_row(&harness, "Ball  (ball-1)");
    let target = egui::pos2(ball.center().x, ball.bottom() - 1.0);

    drag(&mut harness, bands, target);

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "one action queued: {actions:?}");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({
            "moduleId": "bands-1",
            "fromGroupId": "group-a",
            "toGroupId": "group-a",
            "toIndex": 1,
        })
    );

    // The action re-renders in the new order: the engine applies the same
    // remove-then-insert and `modules_in_group` follows `module_ids`.
    apply_move(&snapshot, &actions[0]);
    harness.run();
    let order: Vec<String> = snapshot
        .lock()
        .unwrap()
        .modules_in_group("group-a")
        .iter()
        .map(|module| module.id.clone())
        .collect();
    assert_eq!(order, ["ball-1", "bands-1", "tunnel-1"]);
    let bands = module_row(&harness, "Bands  (bands-1)");
    let ball = module_row(&harness, "Ball  (ball-1)");
    assert!(
        bands.top() > ball.top(),
        "the re-rendered rows follow the new order"
    );
}

#[test]
fn dragging_a_module_above_a_row_in_another_group_inserts_at_that_index() {
    let (mut harness, snapshot) = build();
    let bands = module_grip(&harness, "Drag Bands (bands-1)");
    let plasma = module_row(&harness, "Plasma  (plasma-1)");
    let target = egui::pos2(plasma.center().x, plasma.top() + 1.0);

    drag(&mut harness, bands, target);

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "one action queued: {actions:?}");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({
            "moduleId": "bands-1",
            "fromGroupId": "group-a",
            "toGroupId": "group-b",
            "toIndex": 0,
        })
    );

    apply_move(&snapshot, &actions[0]);
    harness.run();
    let snapshot = snapshot.lock().unwrap();
    assert_eq!(
        snapshot.modules_in_group("group-b")[0].id,
        "bands-1",
        "the dropped module precedes the row it was dropped above"
    );
    assert_eq!(snapshot.modules_in_group("group-a").len(), 2);
}

#[test]
fn dragging_onto_an_empty_group_appends() {
    let (mut harness, snapshot) = build();
    let bands = module_grip(&harness, "Drag Bands (bands-1)");
    let empty = harness.get_by_label("Empty").rect();

    drag(&mut harness, bands, empty.center());

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "one action queued: {actions:?}");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({
            "moduleId": "bands-1",
            "fromGroupId": "group-a",
            "toGroupId": "group-c",
            "toIndex": 0,
        })
    );

    apply_move(&snapshot, &actions[0]);
    harness.run();
    assert_eq!(
        snapshot.lock().unwrap().modules_in_group("group-c")[0].id,
        "bands-1"
    );
}

#[test]
fn dropping_in_the_gap_between_two_rows_inserts_there() {
    // The few pixels between two rows are not inside either row's rect, but
    // they still mean "insert here": moving Bands into the gap between Ball
    // and Tunnel lands it at index 1, not at the end of the Group.
    let (mut harness, snapshot) = build();
    let bands = module_grip(&harness, "Drag Bands (bands-1)");
    let ball = module_row(&harness, "Ball  (ball-1)");
    let tunnel = module_row(&harness, "Tunnel  (tunnel-1)");
    let target = egui::pos2(ball.center().x, (ball.bottom() + tunnel.top()) / 2.0);

    drag(&mut harness, bands, target);

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "one action queued: {actions:?}");
    assert_eq!(
        actions[0].payload["toGroupId"],
        serde_json::json!("group-a")
    );
    assert_eq!(actions[0].payload["toIndex"], serde_json::json!(1));

    apply_move(&snapshot, &actions[0]);
    harness.run();
    let order: Vec<String> = snapshot
        .lock()
        .unwrap()
        .modules_in_group("group-a")
        .iter()
        .map(|module| module.id.clone())
        .collect();
    assert_eq!(order, ["ball-1", "bands-1", "tunnel-1"]);
}

#[test]
fn releasing_a_module_drag_over_empty_space_queues_nothing() {
    let (mut harness, _snapshot) = build();
    let bands = module_grip(&harness, "Drag Bands (bands-1)");
    let empty_space = harness.get_by_label("GALLERY").rect().center();

    drag(&mut harness, bands, empty_space);

    assert!(
        harness.state().actions.is_empty(),
        "released outside a Group: {:?}",
        harness.state().actions
    );
}

#[test]
fn module_rows_no_longer_offer_move_up_or_down_buttons() {
    let (harness, _snapshot) = build();
    let row = module_row(&harness, "Bands  (bands-1)");
    let on_row = |node: &egui_kittest::Node<'_>| {
        let rect = node.rect();
        rect.center().x < 400.0 && (rect.center().y - row.center().y).abs() < 6.0
    };

    assert_eq!(
        harness
            .query_all_by_label("^")
            .filter(|node| on_row(node))
            .count(),
        0,
        "the module row no longer has a Move up button"
    );
    assert_eq!(
        harness
            .query_all_by_label("v")
            .filter(|node| on_row(node))
            .count(),
        0,
        "the module row no longer has a Move down button"
    );
    assert_eq!(
        harness
            .query_all_by_label("D")
            .filter(|node| on_row(node))
            .count(),
        1,
        "the Duplicate button is still on the row"
    );
    assert_eq!(
        harness
            .query_all_by_label("x")
            .filter(|node| on_row(node))
            .count(),
        1,
        "the Remove button is still on the row"
    );
}

#[test]
fn clicking_a_module_row_still_selects_the_module() {
    let (mut harness, _snapshot) = build();
    harness.get_by_label("Bands  (bands-1)").click();
    harness.run();
    assert_eq!(
        harness.state().selected_module.as_deref(),
        Some("bands-1"),
        "the module name is click-to-select"
    );
    assert!(
        harness.state().actions.is_empty(),
        "a click queues nothing: {:?}",
        harness.state().actions
    );
}

#[test]
fn dragging_the_module_name_starts_no_drag() {
    // The two-bar grip is the only drag source. The name is a plain
    // click-to-select button, so pressing it and moving the pointer must not
    // carry a `ModuleDrag` (and therefore queues nothing on release).
    let (mut harness, _snapshot) = build();
    let name = module_row(&harness, "Bands  (bands-1)");
    let ball = module_row(&harness, "Ball  (ball-1)");
    let target = egui::pos2(ball.center().x, ball.bottom() - 1.0);

    drag_hold(&mut harness, name.center(), target);

    assert!(
        egui::DragAndDrop::payload::<modv_ui::module_drag::ModuleDrag>(&harness.ctx).is_none(),
        "dragging the module name carries no ModuleDrag payload"
    );

    release_at(&mut harness, target);

    assert!(
        harness.state().actions.is_empty(),
        "dragging the module name queues nothing: {:?}",
        harness.state().actions
    );
}
