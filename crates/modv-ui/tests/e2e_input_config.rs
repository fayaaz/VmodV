//! Port of `tests/e2e/spec/inputLinks/inputConfigUpdatesWhenControlIsFocused.spec.js`.
//!
//! Legacy `InputConfig.vue` edits only the focused input: clicking a module
//! prop focuses that input and the panel title becomes `<module>: <prop>`.
//! The native shell has no separate Input Config tab; each prop's link row
//! opens the same focused-input editor (`LinkEditor`). These tests drive that
//! path and assert the editor targets the clicked prop's input id.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, GroupView, ModuleView, PropView, UiState};

fn group() -> GroupView {
    GroupView {
        id: "group-1".to_string(),
        name: "Effects".to_string(),
        enabled: true,
        hidden: false,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        inherit: true,
        clearing: false,
        pipeline: false,
        module_ids: vec!["ball-5678".to_string()],
    }
}

fn prop(name: &str, value: f64, input_id: &str) -> PropView {
    PropView {
        name: name.to_string(),
        prop_type: "float".to_string(),
        value: serde_json::json!(value),
        min: Some(0.0),
        max: Some(10.0),
        step: Some(0.001),
        default: None,
        input_id: Some(input_id.to_string()),
        components: Vec::new(),
        tween: None,
        options: Vec::new(),
    }
}

fn ball() -> ModuleView {
    ModuleView {
        id: "ball-5678".to_string(),
        module_name: "Ball".to_string(),
        group_id: "group-1".to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "shader".to_string(),
        props: vec![
            prop("Amount", 1.0, "in-amount"),
            prop("Speed", 2.0, "in-speed"),
        ],
    }
}

fn build() -> Harness<'static, UiState> {
    let snapshot = EngineSnapshot {
        groups: vec![group()],
        modules: vec![ball()],
        ..Default::default()
    };
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            UiState::new(),
        );
    modv_ui::theme::apply(&harness.ctx);
    harness.run();
    harness
}

/// Click the module row in the Groups pane, the native equivalent of clicking
/// the module name in the legacy `Group.vue`.
fn select_ball(harness: &mut Harness<'static, UiState>) {
    harness.get_by_label("Ball  (ball-567)").click();
    harness.run();
    assert_eq!(
        harness.state().selected_module.as_deref(),
        Some("ball-5678"),
        "clicking the module row focuses the module"
    );
}

#[test]
fn focusing_a_prop_opens_the_input_config_for_that_input() {
    let mut harness = build();
    select_ball(&mut harness);

    // The Inspector names the selected module, matching the legacy title
    // assertion.
    assert!(
        harness.get_all_by_label("Ball").count() >= 1,
        "the Inspector shows the module name"
    );

    // Clicking the prop's link row focuses that prop's input. Amount is the
    // first prop, so its `+ link` button is the first one in the Inspector.
    let link = harness
        .get_all_by_label("+ link")
        .next()
        .expect("the first prop has a link row");
    link.click();
    harness.run();

    let editor = harness
        .state()
        .link_editor
        .as_ref()
        .expect("clicking the prop opens the focused-input editor");
    assert_eq!(
        editor.input_id, "in-amount",
        "the editor targets the clicked prop's input, not the module's first input"
    );
    // Native title shape: the shell passes the prop name (legacy InputConfig
    // shows `<module>: <prop>`; the module prefix is not in `GroupView`/the
    // link row today — recorded as a parity gap).
    assert_eq!(editor.title, "Amount");
    harness.get_by_label("Input link: Amount");
}

#[test]
fn the_focused_input_editor_follows_a_different_prop() {
    let mut harness = build();
    select_ball(&mut harness);

    // The second prop's link row must open Speed, not reuse Amount.
    harness
        .get_all_by_label("+ link")
        .nth(1)
        .expect("the second prop has a link row")
        .click();
    harness.run();

    let editor = harness
        .state()
        .link_editor
        .as_ref()
        .expect("clicking the second prop opens the focused-input editor");
    assert_eq!(editor.input_id, "in-speed");
    assert_eq!(editor.title, "Speed");
    harness.get_by_label("Input link: Speed");
}
