//! Interaction coverage for the focused-input editor's native additions:
//! the Group/Module Alpha link row and the Expression section.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{
    EngineSnapshot, ExpressionView, GroupView, InputLinkView, LinkEditor, ModuleView, PropView,
    UiState,
};

fn group() -> GroupView {
    GroupView {
        id: "group-1".to_string(),
        name: "Effects".to_string(),
        enabled: true,
        hidden: false,
        alpha: 1.0,
        alpha_input_id: Some("in-alpha".to_string()),
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        inherit: true,
        clearing: false,
        pipeline: false,
        module_ids: Vec::new(),
    }
}

fn alpha_link() -> InputLinkView {
    InputLinkView {
        input_id: "in-alpha".to_string(),
        kind: "getter".to_string(),
        location: Some("meyda/getFeature".to_string()),
        args: Some(serde_json::json!(["bass"])),
        source: Some("meyda".to_string()),
        min: Some(0.0),
        max: Some(1.0),
        matcher: None,
    }
}

#[test]
fn the_group_alpha_control_gets_a_link_row() {
    let snapshot = EngineSnapshot {
        groups: vec![group()],
        input_links: std::collections::HashMap::from([("in-alpha".to_string(), alpha_link())]),
        ..Default::default()
    };
    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());

    let row_snapshot = snapshot.clone();
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &row_snapshot, state);
            },
            state,
        );
    harness.run();

    // The same Group appears in the Groups pane and the Inspector, so the
    // link row is drawn twice; both are valid.
    assert_eq!(harness.get_all_by_label("LINK").count(), 2);
    assert!(harness.get_all_by_label("Edit").count() >= 2);

    // Open the focused-input editor for the same Group input and check the
    // sections the link row opens.
    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.link_editor = Some(LinkEditor::for_input("in-alpha", "Group alpha", &snapshot));
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    let editor = harness
        .state()
        .link_editor
        .as_ref()
        .expect("editor stays open");
    assert_eq!(editor.input_id, "in-alpha");
    assert_eq!(editor.title, "Group alpha");
    harness.get_by_label("Audio");
    harness.get_by_label("Feature");
    harness.get_by_label("Expression");
}

#[test]
fn the_expression_section_loads_the_assignment_for_the_focused_input() {
    let module = ModuleView {
        id: "module-1".to_string(),
        module_name: "Bands".to_string(),
        group_id: "group-1".to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: Some("in-alpha".to_string()),
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "shader".to_string(),
        props: vec![PropView {
            name: "gain".to_string(),
            prop_type: "float".to_string(),
            value: serde_json::json!(0.5),
            min: Some(0.0),
            max: Some(1.0),
            step: Some(0.001),
            default: None,
            input_id: Some("in-gain".to_string()),
            tween: None,
            options: Vec::new(),
        }],
    };
    let snapshot = EngineSnapshot {
        groups: vec![group()],
        modules: vec![module],
        input_links: std::collections::HashMap::from([("in-alpha".to_string(), alpha_link())]),
        expressions: vec![ExpressionView {
            id: "expr-1".to_string(),
            input_id: "in-gain".to_string(),
            expression: "value * 3".to_string(),
            evaluate_every_frame: false,
        }],
        ..Default::default()
    };
    let mut state = UiState::new();
    state.selected_module = Some("module-1".to_string());
    state.link_editor = Some(LinkEditor::for_input("in-gain", "gain", &snapshot));

    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    assert_eq!(
        harness
            .state()
            .link_editor
            .as_ref()
            .expect("editor stays open")
            .title,
        "gain"
    );
    harness.get_by_label("Expression");
    assert!(
        harness.get_all_by_value("value * 3").count() >= 1,
        "the assignment source is loaded into the textarea"
    );
    harness.get_by_label("Clear expression");
    harness.get_by_label("Evaluate every frame");
}
