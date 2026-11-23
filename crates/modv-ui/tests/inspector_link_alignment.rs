//! Regression test for BUGS.md: "on inspector panel on the right the '+link'
//! button isn't visually aligned with the input prop. it should be in line or
//! have some visual connection."
//!
//! The Inspector currently draws the prop editor and the `+ link` button in
//! two separate `ui.horizontal` rows, so the button has no visual connection
//! to the prop it binds. This test drives the real shell with one prop and
//! asserts the button shares the prop row's vertical band.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, ModuleView, PropView, UiState};

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
        module_ids: vec!["ball-5678".to_string()],
    };
    let module = ModuleView {
        id: "ball-5678".to_string(),
        module_name: "Ball".to_string(),
        group_id: "group-1".to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "shader".to_string(),
        props: vec![PropView {
            name: "Amount".to_string(),
            prop_type: "float".to_string(),
            value: serde_json::json!(1.0),
            min: Some(0.0),
            max: Some(10.0),
            step: Some(0.001),
            default: None,
            input_id: Some("in-amount".to_string()),
            components: Vec::new(),
            tween: None,
            enum_members: Vec::new(),
        }],
    };
    EngineSnapshot {
        groups: vec![group],
        modules: vec![module],
        ..Default::default()
    }
}

#[test]
fn inspector_link_button_shares_the_prop_row() {
    let snapshot = snapshot();
    let mut state = UiState::new();
    state.selected_module = Some("ball-5678".to_string());

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

    // Clicking the module row in the Groups pane switches the right-hand tab
    // strip to the Inspector, as in `e2e_input_config.rs`.
    harness.get_by_label("Ball  (ball-567)").click();
    harness.run();

    // The Inspector shows the float prop's label and the `+ link` control for
    // that same prop. Both must exist before comparing geometry.
    let prop = harness
        .get_all_by_label("Amount")
        .next()
        .expect("the Inspector shows the Amount prop label")
        .rect();
    let link = harness
        .get_all_by_label("+ link")
        .next()
        .expect("the Inspector shows a + link button for the prop")
        .rect();

    // A button drawn on its own row below the editor is not visually aligned
    // with the prop. Requiring the vertical centres to coincide (within a
    // small tolerance for differing widget heights) encodes "in line".
    let dy = (prop.center().y - link.center().y).abs();
    assert!(
        dy <= 10.0,
        "the '+ link' button must be on the same row as the 'Amount' prop control: prop centre y={:.1} (rect {prop:?}), link centre y={:.1} (rect {link:?}), |dy|={dy:.1}",
        prop.center().y,
        link.center().y,
    );
}
