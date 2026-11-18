//! Port of the legacy `tests/e2e/spec/groups/*.spec.js` intents to the native
//! Groups pane, plus `general/defaultGroupIsFocusedByDefault.spec.js` for the
//! startup focus.
//!
//! Each test drives the real shell (so the Groups pane, its selected-Group
//! editor and the Inspector read the same snapshot the engine would) and
//! asserts the exact `groups/*` action the app forwards into the store. The
//! fixture snapshot is mutable: a queued action is applied to it exactly like
//! the engine would apply it, so toggles round-trip (0 -> 1 -> 0) instead of
//! asserting the same stale value twice.
//!
//! Native equivalents of the JS steps are used where the shell has no direct
//! DOM counterpart: the JS `showControls` click is the Group-name click here,
//! and the JS drag-reorder is the native Move up/down buttons. The JS checkbox
//! is tri-state (`Checkbox.vue` with `allow-partial`): a plain click toggles
//! 0/1 and an alt-click selects 2 ("drawn but not to the output canvas").

use std::sync::{Arc, Mutex};

use accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, ModuleView, UiAction, UiState};

fn group(id: &str, name: &str, module_id: &str) -> GroupView {
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
        module_ids: vec![module_id.to_string()],
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

fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        groups: vec![
            group("group-bg", "Background", "bands-1234"),
            group("group-fx", "Effects", "ball-5678"),
        ],
        modules: vec![
            module("bands-1234", "Bands", "group-bg"),
            module("ball-5678", "Ball", "group-fx"),
        ],
        ..Default::default()
    }
}

fn build() -> (Harness<'static, UiState>, Arc<Mutex<EngineSnapshot>>) {
    let snapshot = Arc::new(Mutex::new(snapshot()));
    let draw_snapshot = Arc::clone(&snapshot);
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
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

/// Apply a queued `groups/*` action to the fixture, exactly like the engine
/// store would, so the next frame's widgets read the new value.
fn apply(snapshot: &Arc<Mutex<EngineSnapshot>>, action: &UiAction) {
    let mut snapshot = snapshot.lock().unwrap();
    match action.action.as_str() {
        "groups/UPDATE_GROUP_BY_KEY" => {
            let group_id = action.payload["groupId"].as_str().unwrap_or_default();
            let key = action.payload["key"].as_str().unwrap_or_default();
            let value = &action.payload["value"];
            let Some(group) = snapshot.groups.iter_mut().find(|g| g.id == group_id) else {
                return;
            };
            match key {
                "alpha" => group.alpha = value.as_f64().unwrap_or(group.alpha as f64) as f32,
                "clearing" => group.clearing = value.as_bool().unwrap_or(group.clearing),
                "pipeline" => group.pipeline = value.as_bool().unwrap_or(group.pipeline),
                "inherit" => group.inherit = value.as_bool().unwrap_or(group.inherit),
                "hidden" => group.hidden = value.as_bool().unwrap_or(group.hidden),
                // The raw tri-state value: 0 off, 1 drawn, 2 drawn but not to
                // the output canvas.
                "enabled" => group.enabled = value.as_i64().unwrap_or(group.enabled),
                "inheritFrom" => {
                    group.inherit_from = match value {
                        serde_json::Value::String(id) => InheritTarget::Group(id.clone()),
                        _ => InheritTarget::Previous,
                    }
                }
                "compositeOperation" => {
                    group.composite_operation = value.as_str().unwrap_or_default().to_string()
                }
                _ => {}
            }
        }
        "groups/updateGroupName" => {
            let group_id = action.payload["groupId"].as_str().unwrap_or_default();
            if let Some(group) = snapshot.groups.iter_mut().find(|g| g.id == group_id) {
                group.name = action.payload["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
            }
        }
        "groups/orderByIds" => {
            let ids: Vec<String> = action.payload["ids"]
                .as_array()
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| id.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut reordered = Vec::with_capacity(snapshot.groups.len());
            for id in &ids {
                if let Some(group) = snapshot.groups.iter().find(|g| &g.id == id) {
                    reordered.push(group.clone());
                }
            }
            if reordered.len() == snapshot.groups.len() {
                snapshot.groups = reordered;
            }
        }
        "groups/createGroup" => {
            let id = format!("group-new-{}", snapshot.groups.len());
            snapshot.groups.push(group(&id, "New Group", "module-new"));
        }
        "groups/removeGroup" => {
            let group_id = action.payload["groupId"].as_str().unwrap_or_default();
            snapshot.groups.retain(|g| g.id != group_id);
        }
        _ => {}
    }
}

/// Drain the actions queued by the last frame, apply the Group ones to the
/// fixture and return every queued action.
fn run_and_apply(
    harness: &mut Harness<'static, UiState>,
    snapshot: &Arc<Mutex<EngineSnapshot>>,
) -> Vec<UiAction> {
    // The pointer/key events queued by kittest are processed by this frame.
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

/// A compact button on the Groups-pane row whose Group/Module name is
/// `row_name`. `x`/`v`/`^` repeat on every header and module row, so the
/// controls must be picked by their row rather than by label alone.
fn row_button<'a>(
    harness: &'a Harness<'static, UiState>,
    label: &'static str,
    row_name: &str,
) -> egui_kittest::Node<'a> {
    let row = harness
        .get_all_by_label(row_name)
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .unwrap_or_else(|| panic!("no Groups row named {row_name}"))
        .rect();
    harness
        .get_all_by_label(label)
        .filter(|node| {
            let rect = node.rect();
            rect.center().x < 400.0 && (rect.center().y - row.center().y).abs() < 8.0
        })
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .unwrap_or_else(|| panic!("no {label:?} button on the {row_name} row"))
}

/// Select a Group the way `showControls` does in the legacy UI: click its name
/// in the Groups pane. The Inspector then repeats the name, so the first
/// match (leftmost, in the Groups column) is used.
fn select_group(harness: &mut Harness<'static, UiState>, name: &str) {
    let node = harness
        .get_all_by_label(name)
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .unwrap_or_else(|| panic!("no Group named {name}"));
    node.click();
    harness.run();
    assert_eq!(
        harness.state().selected_group.as_deref(),
        Some(match name {
            "Background" => "group-bg",
            "Effects" => "group-fx",
            _ => panic!("unexpected Group name"),
        }),
        "clicking the Group name selects it and shows its controls"
    );
}

/// The Alpha slider in the Groups pane (the Inspector repeats it for the
/// selected Group, so pick the leftmost one).
fn alpha_slider<'a>(harness: &'a Harness<'static, UiState>) -> egui_kittest::Node<'a> {
    harness
        .get_all_by_role_and_label(Role::Slider, "Alpha")
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the selected Group shows an Alpha slider")
}

/// The tri-state Enabled control on the Background row.
fn enabled_checkbox<'a>(harness: &'a Harness<'static, UiState>) -> egui_kittest::Node<'a> {
    harness
        .get_all_by_label("Enable this Group")
        .next()
        .expect("Background has an enabled checkbox")
}

/// The `inheritFrom` selector under the selected Group's Inherit checkbox.
/// The blend selector is another combo box, so the row is picked by its
/// `Inherit from` label.
fn inherit_combo<'a>(harness: &'a Harness<'static, UiState>) -> egui_kittest::Node<'a> {
    let label = harness.get_by_label("Inherit from").rect();
    harness
        .get_all_by_role(Role::ComboBox)
        .filter(|node| (node.rect().center().y - label.center().y).abs() < 14.0)
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the selected Group exposes the Inherit from selector")
}

/// Open the `inheritFrom` selector and click `option`. The option text is
/// shared with the Group's own header (and the closed combo repeats it), so
/// the matching node closest below the opened combo is the popup row.
fn choose_inherit(harness: &mut Harness<'static, UiState>, option: &str) {
    let combo = inherit_combo(harness);
    let button = combo.rect();
    combo.click();
    harness.run();
    let choice = harness
        .get_all_by_label(option)
        .filter(|node| node.rect().top() >= button.bottom() - 1.0)
        .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .unwrap_or_else(|| panic!("no {option:?} option under the Inherit from selector"));
    choice.click();
    harness.run();
}

#[test]
// `alphaStateCanBeSet.spec.js`.
fn alpha_can_be_set() {
    let (mut harness, snapshot) = build();
    select_group(&mut harness, "Background");

    for value in [0.0, 0.25, 0.75, 0.5, 1.0] {
        alpha_slider(&harness).set_value_accesskit(value);
        let actions = run_and_apply(&mut harness, &snapshot);

        let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
        assert_eq!(updates.len(), 1, "one alpha update: {actions:?}");
        assert_eq!(updates[0].payload["groupId"], "group-bg");
        assert_eq!(updates[0].payload["key"], "alpha");
        let written = updates[0].payload["value"].as_f64().unwrap();
        assert!(
            (written - value).abs() < 0.001,
            "the slider writes {value}, got {written}"
        );

        let alpha = snapshot
            .lock()
            .unwrap()
            .groups
            .iter()
            .find(|group| group.id == "group-bg")
            .unwrap()
            .alpha;
        assert!(
            (alpha - value as f32).abs() < 0.001,
            "the engine value follows the slider: {alpha}"
        );
    }
}

#[test]
// `blendModeStateCanBeSet.spec.js`.
fn blend_mode_can_be_set() {
    let (mut harness, snapshot) = build();
    select_group(&mut harness, "Background");

    for &mode in modv_ui::COMPOSITE_OPERATIONS {
        // The closed combo shows the current mode; opening it lists every
        // COMPOSITE_OPERATIONS entry as a selectable row.
        let selected = snapshot
            .lock()
            .unwrap()
            .groups
            .iter()
            .find(|group| group.id == "group-bg")
            .unwrap()
            .composite_operation
            .clone();
        harness.get_by_value(&selected).click();
        harness.run();
        // The list is taller than the combo popup's max height; scroll the
        // option into view before clicking, like a real user would.
        harness.get_by_label(mode).scroll_to_me();
        harness.run();
        harness.get_by_label(mode).click();

        let actions = run_and_apply(&mut harness, &snapshot);
        let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
        assert_eq!(updates.len(), 1, "one blend update for {mode}: {actions:?}");
        assert_eq!(updates[0].payload["groupId"], "group-bg");
        assert_eq!(updates[0].payload["key"], "compositeOperation");
        assert_eq!(updates[0].payload["value"], mode);

        let current = snapshot
            .lock()
            .unwrap()
            .groups
            .iter()
            .find(|group| group.id == "group-bg")
            .unwrap()
            .composite_operation
            .clone();
        assert_eq!(current, mode);
    }
}

#[test]
// `clearingStateCanBeToggledBetween0and1.spec.js`.
fn clearing_state_can_be_toggled_between_0_and_1() {
    let (mut harness, snapshot) = build();
    select_group(&mut harness, "Background");

    harness.get_by_label("Clear").click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one clearing update: {actions:?}");
    assert_eq!(updates[0].payload["key"], "clearing");
    assert_eq!(updates[0].payload["value"], serde_json::json!(true));
    assert!(
        snapshot.lock().unwrap().groups[0].clearing,
        "the first click enables clearing"
    );

    harness.get_by_label("Clear").click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one clearing update: {actions:?}");
    assert_eq!(updates[0].payload["key"], "clearing");
    assert_eq!(updates[0].payload["value"], serde_json::json!(false));
    assert!(
        !snapshot.lock().unwrap().groups[0].clearing,
        "the second click disables clearing"
    );
}

#[test]
// `enabledStateCanBeToggledBetween01and2.spec.js`.
fn enabled_state_can_be_toggled_between_0_1_and_2() {
    let (mut harness, snapshot) = build();

    // 1 -> 0: the checkbox's plain click disables the Group.
    enabled_checkbox(&harness).click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one enabled update: {actions:?}");
    assert_eq!(updates[0].payload["groupId"], "group-bg");
    assert_eq!(updates[0].payload["key"], "enabled");
    assert_eq!(updates[0].payload["value"], serde_json::json!(0));
    assert_eq!(snapshot.lock().unwrap().groups[0].enabled, 0);

    // 0 -> 1: the next click draws it again.
    enabled_checkbox(&harness).click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one enabled update: {actions:?}");
    assert_eq!(updates[0].payload["value"], serde_json::json!(1));
    assert_eq!(snapshot.lock().unwrap().groups[0].enabled, 1);

    // 1 -> 2: alt-click is the partial state (drawn, output canvas skipped).
    enabled_checkbox(&harness).click_modifiers(egui::Modifiers::ALT);
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one enabled update: {actions:?}");
    assert_eq!(updates[0].payload["value"], serde_json::json!(2));
    let group = snapshot.lock().unwrap().groups[0].clone();
    assert_eq!(group.enabled, 2);
    assert!(group.is_drawn(), "2 is still drawn");
    assert!(
        !group.draws_to_output(),
        "2 is not drawn to the output canvas"
    );
    assert_eq!(
        modv_ui::model::group_enabled_indicator(group.enabled),
        "partial",
        "the control shows the partial indicator"
    );

    // 2 -> 0: a plain click from partial disables it, like `Checkbox.vue`.
    enabled_checkbox(&harness).click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one enabled update: {actions:?}");
    assert_eq!(updates[0].payload["value"], serde_json::json!(0));
    assert_eq!(snapshot.lock().unwrap().groups[0].enabled, 0);

    // 0 -> 1 again.
    enabled_checkbox(&harness).click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one enabled update: {actions:?}");
    assert_eq!(updates[0].payload["value"], serde_json::json!(1));
    assert_eq!(snapshot.lock().unwrap().groups[0].enabled, 1);
}

#[test]
// `groupNameCanBeChanged.spec.js`.
fn group_name_can_be_changed() {
    let (mut harness, snapshot) = build();
    select_group(&mut harness, "Background");

    // The selected Group's name field is already the inline editor (the
    // legacy UI reveals the same input on a header double-click).
    let field = harness
        .get_all_by_role(Role::TextInput)
        .find(|node| node.value().as_deref() == Some("Background"))
        .expect("the selected Group's name field holds the current name");
    field.focus();
    field.set_text_value_accesskit("Post FX");
    harness.run();
    harness.key_press(egui::Key::Enter);

    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/updateGroupName");
    assert_eq!(updates.len(), 1, "one rename action: {actions:?}");
    assert_eq!(updates[0].payload["groupId"], "group-bg");
    assert_eq!(updates[0].payload["name"], "Post FX");
    assert_eq!(snapshot.lock().unwrap().groups[0].name, "Post FX");
}

#[test]
// `inheritAndInheritFromStateCanBeSet.spec.js`.
fn inherit_and_inherit_from_can_be_set() {
    let (mut harness, snapshot) = build();
    select_group(&mut harness, "Background");

    // The selector starts on the engine default: the previous Group.
    assert_eq!(
        snapshot.lock().unwrap().groups[0].inherit_from,
        InheritTarget::Previous
    );
    assert_eq!(
        harness.get_all_by_value("Previous Group").count(),
        1,
        "the closed selector shows the previous Group target"
    );

    // Pick another Group by name: `groups/UPDATE_GROUP_BY_KEY` carries its
    // UUID string, the shape `groups.js` stores in `inheritFrom`.
    choose_inherit(&mut harness, "Effects");
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one inheritFrom update: {actions:?}");
    assert_eq!(updates[0].payload["groupId"], "group-bg");
    assert_eq!(updates[0].payload["key"], "inheritFrom");
    assert_eq!(updates[0].payload["value"], serde_json::json!("group-fx"));
    assert_eq!(
        snapshot.lock().unwrap().groups[0].inherit_from,
        InheritTarget::Group("group-fx".to_string())
    );
    assert_eq!(
        harness.get_all_by_value("Effects").count(),
        1,
        "the selector now shows the named Group"
    );

    // Back to the numeric `-1` form.
    choose_inherit(&mut harness, "Previous Group");
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one inheritFrom update: {actions:?}");
    assert_eq!(updates[0].payload["key"], "inheritFrom");
    assert_eq!(updates[0].payload["value"], serde_json::json!(-1));
    assert_eq!(
        snapshot.lock().unwrap().groups[0].inherit_from,
        InheritTarget::Previous
    );

    // `Don't Inherit` is the Inherit checkbox: turning it off hides the
    // selector, turning it back on restores it.
    harness.get_by_label("Inherit").click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one inherit update: {actions:?}");
    assert_eq!(updates[0].payload["key"], "inherit");
    assert_eq!(updates[0].payload["value"], serde_json::json!(false));
    assert!(!snapshot.lock().unwrap().groups[0].inherit);
    assert_eq!(
        harness.query_all_by_label("Inherit from").count(),
        0,
        "the target selector is hidden while inheritance is off"
    );

    harness.get_by_label("Inherit").click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one inherit update: {actions:?}");
    assert_eq!(updates[0].payload["value"], serde_json::json!(true));
    assert!(snapshot.lock().unwrap().groups[0].inherit);
    harness.get_by_label("Inherit from");
}

#[test]
// `pipelineStateCanBeToggledBetween0and1.spec.js`.
fn pipeline_state_can_be_toggled_between_0_and_1() {
    let (mut harness, snapshot) = build();
    select_group(&mut harness, "Background");

    harness.get_by_label("Pipeline").click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one pipeline update: {actions:?}");
    assert_eq!(updates[0].payload["key"], "pipeline");
    assert_eq!(updates[0].payload["value"], serde_json::json!(true));
    assert!(snapshot.lock().unwrap().groups[0].pipeline);

    harness.get_by_label("Pipeline").click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/UPDATE_GROUP_BY_KEY");
    assert_eq!(updates.len(), 1, "one pipeline update: {actions:?}");
    assert_eq!(updates[0].payload["value"], serde_json::json!(false));
    assert!(!snapshot.lock().unwrap().groups[0].pipeline);
}

#[test]
fn groups_can_be_rearranged() {
    let (mut harness, snapshot) = build();

    // The native pane reorders with the per-Group Move up/down buttons rather
    // than a drag. The down button on the first Group swaps it with the
    // second, exactly like `groupsCanBeRearranged.spec.js`'s drop.
    row_button(&harness, "v", "Background").click();
    let actions = run_and_apply(&mut harness, &snapshot);

    let updates = find(&actions, "groups/orderByIds");
    assert_eq!(updates.len(), 1, "one order action: {actions:?}");
    assert_eq!(
        updates[0].payload["ids"],
        serde_json::json!(["group-fx", "group-bg"])
    );
    let names: Vec<String> = snapshot
        .lock()
        .unwrap()
        .groups
        .iter()
        .map(|group| group.name.clone())
        .collect();
    assert_eq!(names, ["Effects", "Background"]);

    // Move it back up: the Group headers follow the new order, so Background
    // is now the second row and its up button swaps it back.
    row_button(&harness, "^", "Background").click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let updates = find(&actions, "groups/orderByIds");
    assert_eq!(updates.len(), 1, "one order action: {actions:?}");
    assert_eq!(
        updates[0].payload["ids"],
        serde_json::json!(["group-bg", "group-fx"])
    );
}

#[test]
// `backspaceRemovesFocusedGroup.spec.js`.
fn backspace_removes_the_focused_group() {
    let (mut harness, snapshot) = build();
    select_group(&mut harness, "Effects");

    harness.key_press(egui::Key::Backspace);
    let actions = run_and_apply(&mut harness, &snapshot);

    let removes = find(&actions, "groups/removeGroup");
    assert_eq!(removes.len(), 1, "one removal: {actions:?}");
    assert_eq!(removes[0].payload["groupId"], "group-fx");
    assert_eq!(snapshot.lock().unwrap().groups.len(), 1);
    assert!(
        harness.state().selected_group.is_none(),
        "removing the focused Group clears the selection"
    );

    // The per-Group Remove button stays as the pointer path to the same
    // action.
    select_group(&mut harness, "Background");
    row_button(&harness, "x", "Background").click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let removes = find(&actions, "groups/removeGroup");
    assert_eq!(removes.len(), 1, "one removal: {actions:?}");
    assert!(snapshot.lock().unwrap().groups.is_empty());
}

#[test]
fn backspace_is_ignored_while_a_text_field_has_focus() {
    let (mut harness, snapshot) = build();
    select_group(&mut harness, "Background");

    let field = harness
        .get_all_by_role(Role::TextInput)
        .find(|node| node.value().as_deref() == Some("Background"))
        .expect("the selected Group's name field is a text input");
    field.focus();
    harness.run();

    harness.key_press(egui::Key::Backspace);
    let actions = run_and_apply(&mut harness, &snapshot);
    assert!(
        find(&actions, "groups/removeGroup").is_empty(),
        "Backspace edits text instead of removing the Group: {actions:?}"
    );
    assert_eq!(snapshot.lock().unwrap().groups.len(), 2);
    assert_eq!(harness.state().selected_group.as_deref(), Some("group-bg"));
}

#[test]
// `newGroupButtonCreatesANewGroup.spec.js`.
fn the_new_group_button_creates_a_group() {
    let (mut harness, snapshot) = build();

    harness.get_by_label("+ Group").click();
    let actions = run_and_apply(&mut harness, &snapshot);

    let creates = find(&actions, "groups/createGroup");
    assert_eq!(creates.len(), 1, "one create action: {actions:?}");
    assert_eq!(creates[0].payload, serde_json::json!({}));
    assert_eq!(snapshot.lock().unwrap().groups.len(), 3);

    // The header `+` button queues the same payload.
    harness
        .get_all_by_label("+")
        .find(|node| node.rect().left() < 400.0)
        .expect("the Groups header has a + button")
        .click();
    let actions = run_and_apply(&mut harness, &snapshot);
    let creates = find(&actions, "groups/createGroup");
    assert_eq!(creates.len(), 1, "one create action: {actions:?}");
    assert_eq!(snapshot.lock().unwrap().groups.len(), 4);
}

#[test]
// `tests/e2e/spec/general/defaultGroupIsFocusedByDefault.spec.js`.
fn the_default_group_is_focused_by_default() {
    // The first Group carries the focus indicator and `focus.id` names it. A
    // fresh `UiState` starts with no selection, so the shell focuses the first
    // user Group on startup.
    let (harness, _snapshot) = build();

    assert_eq!(
        harness.state().selected_group.as_deref(),
        Some("group-bg"),
        "the first live Group is focused on startup"
    );
}
