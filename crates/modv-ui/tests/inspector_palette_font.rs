//! Feature 4: palette and font prop values must be editable in the Inspector.
//!
//! Today `draw_prop` renders the `font` prop as a read-only `ui.label` and the
//! `palette` prop as a row of painted, non-interactive swatches. Both queue no
//! action. These tests drive the real shell with one module carrying a `font`
//! and a `palette` prop and require accessible text fields whose current
//! values are the font family and each colour's `#RRGGBB` hex. Editing those
//! fields must queue `modules/updateProp` actions with the new values.
//!
//! Harness/fixture based on `tests/inspector_link_alignment.rs` and
//! `tests/e2e_input_config.rs`; the text-input interaction follows
//! `tests/plugins_panel.rs` and `tests/e2e_groups.rs`.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, ModuleView, PropView, UiState};

const MODULE_ID: &str = "ball-5678";

fn snapshot() -> EngineSnapshot {
    let group = GroupView {
        id: "group-1".to_string(),
        name: "Effects".to_string(),
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
        module_ids: vec![MODULE_ID.to_string()],
    };
    let module = ModuleView {
        id: MODULE_ID.to_string(),
        module_name: "Ball".to_string(),
        group_id: "group-1".to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "shader".to_string(),
        props: vec![
            PropView {
                name: "Font".to_string(),
                prop_type: "font".to_string(),
                value: serde_json::json!("Inter"),
                min: None,
                max: None,
                step: None,
                default: None,
                input_id: None,
                components: Vec::new(),
                tween: None,
                enum_members: Vec::new(),
            },
            PropView {
                name: "Palette".to_string(),
                prop_type: "palette".to_string(),
                value: serde_json::json!([
                    { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                    { "r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0 },
                ]),
                min: None,
                max: None,
                step: None,
                default: None,
                input_id: None,
                components: Vec::new(),
                tween: None,
                enum_members: Vec::new(),
            },
        ],
    };
    EngineSnapshot {
        groups: vec![group],
        modules: vec![module],
        ..Default::default()
    }
}

fn harness() -> Harness<'static, UiState> {
    let snapshot = snapshot();
    let mut state = UiState::new();
    state.selected_group = Some("group-1".to_string());
    state.selected_module = Some(MODULE_ID.to_string());
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness.run();
    // Clicking the module row in Groups focuses the Inspector tab, as in the
    // reference harnesses.
    harness.get_by_label("Ball  (ball-567)").click();
    harness.run();
    harness
}

fn prop_updates(harness: &mut Harness<'_, UiState>, prop: &str) -> Vec<modv_ui::UiAction> {
    harness
        .state_mut()
        .take_actions()
        .into_iter()
        .filter(|action| action.action == "modules/updateProp" && action.payload["prop"] == prop)
        .collect()
}

fn close(actual: Option<f64>, expected: f64, label: &str) {
    let actual = actual.unwrap_or(f64::NAN);
    assert!(
        (actual - expected).abs() <= 0.02,
        "{label}: expected {expected}, got {actual}"
    );
}

#[test]
fn font_prop_is_an_editable_text_field() {
    let mut harness = harness();

    // The font family must be exposed as an accesskit TextInput carrying its
    // current value, so assistive tech (and this test) can find it by value.
    let field = harness
        .get_all_by_role(accesskit::Role::TextInput)
        .find(|node| node.value().as_deref() == Some("Inter"))
        .expect("the Font prop renders an editable TextInput showing 'Inter'");
    field.focus();
    // `type_text` may append at the cursor; that is fine, the emitted value
    // only has to end with the newly typed family.
    field.type_text("IBMPlex");
    harness.run();

    let updates = prop_updates(&mut harness, "Font");
    assert_eq!(
        updates.len(),
        1,
        "editing the font family queues exactly one modules/updateProp for Font: {updates:?}"
    );
    let value = &updates[0].payload["data"]["value"];
    let text = value
        .as_str()
        .expect("the font prop payload carries a string value");
    assert!(
        text.ends_with("IBMPlex"),
        "the queued font value must end with the typed family 'IBMPlex', got {text:?}"
    );
    assert_eq!(updates[0].payload["moduleId"], MODULE_ID);
}

#[test]
fn palette_swatches_have_editable_hex_fields() {
    let mut harness = harness();

    // Every palette entry must expose an editable hex field whose current
    // value is the entry's `#RRGGBB`. The first entry is red in the fixture.
    let field = harness
        .get_all_by_role(accesskit::Role::TextInput)
        .find(|node| node.value().as_deref() == Some("#FF0000"))
        .expect("the Palette prop exposes an editable hex TextInput showing '#FF0000'");
    field.focus();
    field.set_text_value_accesskit("#00FF00");
    harness.run();

    let updates = prop_updates(&mut harness, "Palette");
    assert_eq!(
        updates.len(),
        1,
        "editing a swatch queues exactly one modules/updateProp for Palette: {updates:?}"
    );
    let value = &updates[0].payload["data"]["value"];
    let palette = value
        .as_array()
        .expect("the palette prop payload is the whole array");
    assert_eq!(
        palette.len(),
        2,
        "both palette entries are preserved: {value}"
    );
    assert_eq!(updates[0].payload["moduleId"], MODULE_ID);

    // Only entry 0 changed (red -> green); entry 1 was already green and must
    // be preserved, along with the original object shape (ratio floats).
    let first = &palette[0];
    assert!(
        first.is_object(),
        "palette entries keep their r/g/b/a object shape: {first}"
    );
    close(first["r"].as_f64(), 0.0, "entry 0 r");
    close(first["g"].as_f64(), 1.0, "entry 0 g");
    close(first["b"].as_f64(), 0.0, "entry 0 b");
    close(first["a"].as_f64(), 1.0, "entry 0 a");

    let second = &palette[1];
    assert!(
        second.is_object(),
        "the untouched entry keeps its shape: {second}"
    );
    close(second["r"].as_f64(), 0.0, "entry 1 r");
    close(second["g"].as_f64(), 1.0, "entry 1 g");
    close(second["b"].as_f64(), 0.0, "entry 1 b");
    close(second["a"].as_f64(), 1.0, "entry 1 a");
}

#[test]
fn rendering_without_editing_queues_no_prop_action() {
    let mut harness = harness();

    // Focusing a control without changing its text must not emit a write; if
    // the editable fields are not there yet, rendering alone still must not.
    if let Some(field) = harness
        .get_all_by_role(accesskit::Role::TextInput)
        .find(|node| node.value().as_deref() == Some("Inter"))
    {
        field.focus();
    }
    harness.run();
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert!(
        actions
            .iter()
            .all(|action| action.action != "modules/updateProp"),
        "rendering/focusing without editing must not queue a prop update: {actions:?}"
    );
}
