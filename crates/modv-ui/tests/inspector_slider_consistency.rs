//! Regression test: adding an input link must not shrink the prop's slider.
//!
//! `draw_prop` draws an unlinked numeric prop with `draw_with_trailing` (the
//! compact `+ link` button shares the row), but once a link exists it switches
//! to `draw`. `RangeControl::draw_with_trailing` overrides
//! `spacing.slider_width` with a width computed from the row's available space,
//! while `draw` leaves egui's theme default (140px) in place. Since egui 0.37
//! sizes sliders from `spacing.slider_width` and ignores `add_sized`, the linked
//! slider ends up noticeably narrower than its unlinked siblings.

use std::cell::RefCell;
use std::rc::Rc;

use accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, InputLinkView, ModuleView, PropView, UiState};

fn float_prop(name: &str, input_id: &str) -> PropView {
    PropView {
        name: name.to_string(),
        prop_type: "float".to_string(),
        value: serde_json::json!(1.0),
        min: Some(0.0),
        max: Some(10.0),
        step: Some(0.001),
        default: None,
        input_id: Some(input_id.to_string()),
        components: Vec::new(),
        tween: None,
        enum_members: Vec::new(),
    }
}

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
        // Two sibling float props, both with an input id, so before the link
        // they are rendered by the exact same `draw_with_trailing` path.
        props: vec![
            float_prop("Amount", "in-amount"),
            float_prop("Width", "in-width"),
        ],
    };
    EngineSnapshot {
        groups: vec![group],
        modules: vec![module],
        ..Default::default()
    }
}

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

/// The width of the slider drawn on the same Inspector row as `prop_label`.
///
/// The `RangeControl` slider has no accessible text of its own, so it is found
/// by its `Slider` role together with its position relative to the prop label.
fn slider_width_for(harness: &Harness<'_, UiState>, prop_label: &str) -> f32 {
    let label = harness
        .get_all_by_label(prop_label)
        .next()
        .unwrap_or_else(|| panic!("the Inspector shows the {prop_label} prop label"))
        .rect();
    harness
        .get_all_by_role(Role::Slider)
        .map(|node| node.rect())
        .find(|rect| {
            (rect.center().y - label.center().y).abs() <= 12.0
                && rect.left() >= label.right()
                && rect.left() > 900.0 // the Inspector sits on the right
        })
        .unwrap_or_else(|| panic!("the Inspector shows a slider on the {prop_label} row"))
        .width()
}

#[test]
fn adding_a_link_keeps_the_prop_slider_as_wide_as_its_siblings() {
    let snapshot = Rc::new(RefCell::new(snapshot()));
    let ui_snapshot = snapshot.clone();
    let mut state = UiState::new();
    state.selected_module = Some("ball-5678".to_string());
    // Widen the Inspector from its 340px default so the unlinked slider is
    // sized from the row's available width to more than egui's fixed 140px
    // `spacing.slider_width`. That is the exact case where switching the
    // linked prop from `draw_with_trailing` to `draw` shrinks its slider.
    state.layout.inspector_width = 600.0;

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

    // Selecting the module in the Groups pane focuses the Inspector tab.
    harness.get_by_label("Ball  (ball-567)").click();
    harness.run();
    harness.run();

    // Sanity: before any link, both props travel the same code path and their
    // sliders must be the same width.
    let amount_before = slider_width_for(&harness, "Amount");
    let width_before = slider_width_for(&harness, "Width");
    assert!(
        (amount_before - width_before).abs() <= 2.0,
        "before linking, sibling sliders match: Amount={amount_before}, Width={width_before}"
    );

    // Simulate the Save/Create-link flow echoing back through the engine: the
    // Amount prop now carries an input link and `draw_prop` switches it to
    // `control.draw(ui)` (no trailing widget).
    snapshot
        .borrow_mut()
        .input_links
        .insert("in-amount".to_string(), amount_link());
    harness.run();
    harness.run();

    harness
        .get_all_by_label("LINKED")
        .next()
        .expect("the linked row renders");

    let amount_after = slider_width_for(&harness, "Amount");
    let width_after = slider_width_for(&harness, "Width");
    assert!(
        amount_after + 2.0 >= width_after,
        "adding a link must not shrink the prop slider below its unlinked sibling: \
         Amount (linked)={amount_after}, Width (unlinked)={width_after}, \
         before: Amount={amount_before}, Width={width_before}"
    );
}
