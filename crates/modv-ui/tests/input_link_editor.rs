//! Interaction coverage for the focused-input editor's VmodV additions:
//! the Group/Module Alpha link row and the Expression section.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{
    EngineSnapshot, ExpressionView, GroupView, InputLinkView, LinkEditor, ModuleView,
    PropComponentView, PropView, UiState,
};

fn group() -> GroupView {
    GroupView {
        id: "group-1".to_string(),
        name: "Effects".to_string(),
        enabled: GROUP_ENABLED,
        hidden: false,
        alpha: 1.0,
        alpha_input_id: Some("in-alpha".to_string()),
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        inherit: true,
        inherit_from: InheritTarget::Previous,
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
            components: Vec::new(),
            tween: None,
            enum_members: Vec::new(),
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

#[test]
fn the_audio_section_saves_a_bpm_link_for_the_engine() {
    let module = ModuleView {
        id: "module-1".to_string(),
        module_name: "rave.exe".to_string(),
        group_id: "group-1".to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: Some("in-alpha".to_string()),
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "isf".to_string(),
        props: vec![PropView {
            name: "bpm".to_string(),
            prop_type: "float".to_string(),
            value: serde_json::json!(140.0),
            min: Some(60.0),
            max: Some(200.0),
            step: Some(1.0),
            default: None,
            input_id: Some("in-bpm".to_string()),
            components: Vec::new(),
            tween: None,
            enum_members: Vec::new(),
        }],
    };
    let snapshot = EngineSnapshot {
        groups: vec![group()],
        modules: vec![module],
        input_links: std::collections::HashMap::from([(
            "in-bpm".to_string(),
            InputLinkView {
                input_id: "in-bpm".to_string(),
                kind: "getter".to_string(),
                location: Some("meyda/getFeature".to_string()),
                args: Some(serde_json::json!(["bass"])),
                source: Some("meyda".to_string()),
                min: Some(0.0),
                max: Some(1.0),
                matcher: None,
            },
        )]),
        ..Default::default()
    };

    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    // The Audio feature picker's "bpm" entry does exactly this.
    let mut editor = LinkEditor::for_input("in-bpm", "BPM", &snapshot);
    editor.set_audio_feature("bpm");
    state.link_editor = Some(editor);

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

    harness.get_by_label("Audio");
    harness.get_by_label("Feature");
    assert!(
        harness
            .get_all_by_label(
                "BPM is a state link on beats.bpm; it waits for a detected tempo (no smoothing).",
            )
            .count()
            >= 1,
        "the Audio section explains the BPM link"
    );

    harness.get_by_label("Save link").click();
    harness.run();

    let action = harness
        .state()
        .actions
        .iter()
        .find(|action| action.action == "inputs/createInputLink")
        .expect("Save queues the engine link action");
    assert_eq!(action.payload["type"], "state");
    assert_eq!(action.payload["source"], "bpm");
    assert_eq!(action.payload["location"], "beats.bpm");
    assert_eq!(action.payload["min"], 0.0);
    assert_eq!(action.payload["max"], 1.0);
}

#[test]
fn the_draw_section_creates_a_curve_and_a_draw_link() {
    let module = ModuleView {
        id: "module-1".to_string(),
        module_name: "Bands".to_string(),
        group_id: "group-1".to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "shader".to_string(),
        props: vec![PropView {
            name: "gain".to_string(),
            prop_type: "float".to_string(),
            value: serde_json::json!(1.0),
            min: Some(0.0),
            max: Some(2.0),
            step: Some(0.001),
            default: None,
            input_id: Some("in-draw".to_string()),
            components: Vec::new(),
            tween: None,
            enum_members: Vec::new(),
        }],
    };
    let snapshot = EngineSnapshot {
        groups: vec![group()],
        modules: vec![module],
        ..Default::default()
    };

    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    state.link_editor = Some(LinkEditor::for_input("in-draw", "gain", &snapshot));

    let draw_snapshot = snapshot.clone();
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &draw_snapshot, state);
            },
            state,
        );
    harness.run();

    harness.get_by_label("Draw").click();
    harness.run();
    harness.get_by_label("Start drawing").click();
    harness.run();

    let editor = harness
        .state()
        .link_editor
        .as_ref()
        .expect("the link editor stays open");
    assert_eq!(editor.source, "draw");
    assert_eq!(editor.kind, "state");
    assert_eq!(editor.location, "tweens.tweens['in-draw'].value[0]");
    assert!(editor.tween.is_some());
    assert!(
        harness.state().draw_editor.is_some(),
        "the freehand canvas opens"
    );

    let actions = &harness.state_mut().take_actions();
    assert_eq!(actions[0].action, "tweens/createTween");
    assert_eq!(actions[0].payload["easing"], "linear");
    assert_eq!(
        actions[0].payload["data"][0],
        serde_json::json!([1.0]),
        "the flat starting line uses the prop range, not the link's 0..1 bounds"
    );
    assert_eq!(actions[1].action, "inputs/createInputLink");
    assert_eq!(actions[1].payload["source"], "draw");
    assert_eq!(actions[1].payload["min"], 0);
    assert_eq!(actions[1].payload["max"], 1);
}

#[test]
fn component_sub_inputs_get_link_rows_under_their_parent_prop() {
    // A color prop owns four `${parent}-${key}` sub-inputs
    // (`initialiseModuleProperties`, `modules.js:91-113`); the Inspector
    // lists each component under the parent, like the JS ColorControl's
    // focused inputs.
    let module = ModuleView {
        id: "module-1".to_string(),
        module_name: "Text".to_string(),
        group_id: "group-1".to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "2d".to_string(),
        props: vec![PropView {
            name: "fillColor".to_string(),
            prop_type: "color".to_string(),
            value: serde_json::json!({"r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0}),
            min: None,
            max: None,
            step: None,
            default: None,
            input_id: Some("in-fill".to_string()),
            components: ["r", "g", "b", "a"]
                .iter()
                .map(|key| PropComponentView {
                    key: (*key).to_string(),
                    input_id: format!("in-fill-{key}"),
                })
                .collect(),
            tween: None,
            enum_members: Vec::new(),
        }],
    };
    let mut snapshot = EngineSnapshot {
        groups: vec![group()],
        modules: vec![module],
        input_links: std::collections::HashMap::from([(
            "in-fill-r".to_string(),
            InputLinkView {
                input_id: "in-fill-r".to_string(),
                kind: "getter".to_string(),
                location: Some("meyda/getFeature".to_string()),
                args: Some(serde_json::json!(["bass"])),
                source: Some("meyda".to_string()),
                min: Some(0.0),
                max: Some(1.0),
                matcher: None,
            },
        )]),
        ..Default::default()
    };
    snapshot.modules[0].group_id = "group-1".to_string();

    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());

    let draw_snapshot = snapshot.clone();
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &draw_snapshot, state);
            },
            state,
        );
    harness.run();

    // Only the bound `r` component carries a link in this fixture, so a
    // single LINK badge proves the component row is drawn (without the
    // expansion the badge count is zero — the parent itself is unlinked).
    assert_eq!(
        harness.get_all_by_label("LINK").count(),
        1,
        "the linked component row carries the LINK badge"
    );
    // Opening the component editor keeps the JS title shape
    // (`${inputTitle}.${append}`).
    let editor_state = harness.state();
    assert!(editor_state.link_editor.is_none());
    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some("module-1".to_string());
    state.link_editor = Some(LinkEditor::for_input("in-fill-g", "fillColor.g", &snapshot));
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
        .expect("the component editor stays open");
    assert_eq!(editor.input_id, "in-fill-g");
    assert_eq!(editor.title, "fillColor.g");
    harness.get_by_label("Audio");
    harness.get_by_label("Feature");
}
