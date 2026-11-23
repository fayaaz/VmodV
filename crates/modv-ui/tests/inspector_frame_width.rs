//! Regression test: adding an input link must not widen the Inspector frame.
//!
//! The Inspector panel is bounded by `state.layout.inspector_width` (default
//! 340, minimum 250). Once a prop carries an input link, its inline link row
//! is longer than the `+ link` button, so the panel's content can demand more
//! width than the frame was allocated. The shell must keep the frame at its
//! configured size instead of recording the content-expanded rect back into
//! the layout (which would make the panel swallow the freed space).

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, InputLinkView, ModuleView, PropView, UiState};

fn snapshot_without_link() -> EngineSnapshot {
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

/// The link an audio link editor creates for the prop's input.
fn amount_link() -> InputLinkView {
    InputLinkView {
        input_id: "in-amount".to_string(),
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
fn adding_a_link_keeps_the_inspector_frame_bounded() {
    let snapshot = Rc::new(RefCell::new(snapshot_without_link()));
    let ui_snapshot = snapshot.clone();
    let mut state = UiState::new();
    state.selected_module = Some("ball-5678".to_string());

    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let snapshot = ui_snapshot.borrow();
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness.run();

    // Select the module through the Groups pane, which also focuses the
    // Inspector tab (as in `inspector_link_alignment.rs`).
    harness.get_by_label("Ball  (ball-567)").click();
    harness.run();

    // The unlinked prop's Inspector row fits the configured frame, and the
    // `+ link` control is present before the add-link flow starts.
    harness.get_by_label("+ link");
    let before = harness.state().layout.inspector_width;
    assert!(
        (250.0..=400.0).contains(&before),
        "the Inspector starts at its configured, bounded width: {before}"
    );

    // Simulate the engine echoing back the link created by the editor: the
    // prop now carries an input link and its inline row shows LINK/summary/Edit.
    snapshot
        .borrow_mut()
        .input_links
        .insert("in-amount".to_string(), amount_link());
    harness.run();

    harness.get_by_label("LINKED");
    let after = harness.state().layout.inspector_width;
    assert!(
        (after - before).abs() <= 2.0,
        "adding a link must not change the Inspector frame width: {before} -> {after} (configured 340.0, minimum 250.0)"
    );
    assert!(
        after <= 400.0,
        "the Inspector frame keeps its bounded width after adding a link: {after}"
    );
}
