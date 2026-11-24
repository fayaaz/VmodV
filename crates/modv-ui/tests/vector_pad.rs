//! The Inspector's vec2 control must expose an interactive 2D vector pad.
//!
//! This test drives the real shell with a vec2 prop and requires the first two lanes
//! to be editable by dragging inside a square pad: the gesture must queue the
//! same `modules/updateProp` action the numeric rows use, while the accessible
//! numeric lanes remain available for keyboard users.
//!
//! Harness/fixture based on `tests/inspector_link_alignment.rs` and
//! `tests/e2e_input_config.rs`; the drag sequence follows
//! `tests/tween_curve_editor.rs`.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::{EngineSnapshot, GroupView, ModuleView, PropComponentView, PropView, UiState};

const MIN: f64 = -1.0;
const MAX: f64 = 1.0;

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
            name: "Position".to_string(),
            prop_type: "vec2".to_string(),
            value: serde_json::json!([0.0, 0.0]),
            min: Some(MIN),
            max: Some(MAX),
            step: Some(0.001),
            default: None,
            input_id: Some("in-position".to_string()),
            components: vec![
                PropComponentView {
                    key: "0".to_string(),
                    input_id: "in-position-0".to_string(),
                },
                PropComponentView {
                    key: "1".to_string(),
                    input_id: "in-position-1".to_string(),
                },
            ],
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

fn harness() -> Harness<'static, UiState> {
    let snapshot = snapshot();
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
    // Clicking the module row in Groups focuses the Inspector tab, as in
    // `inspector_link_alignment.rs`.
    harness.get_by_label("Ball  (ball-567)").click();
    harness.run();
    harness
}

fn close(actual: Option<f64>, expected: f64, lane: &str) {
    let actual = actual.unwrap_or(f64::NAN);
    assert!(
        (actual - expected).abs() <= 0.06,
        "{lane} lane after the pad drag should be about {expected}, got {actual}"
    );
}

#[test]
fn inspector_vector_pad_drag_updates_both_lanes_and_keeps_numeric_fields() {
    let mut harness = harness();

    // The pad is addressed through accesskit, so assistive tech (and this
    // test) can find it without relying on painted pixels.
    let pad = harness
        .query_all_by_label_contains("Vector pad")
        .next()
        .expect("the Inspector's vec2 control exposes an accesskit label containing 'Vector pad'")
        .rect();
    assert!(
        pad.width() >= 40.0 && pad.height() >= 40.0,
        "the vector pad must be an interactive square, got {pad:?}"
    );

    // Press at the centre and drag to 75% across / 25% down the pad. With
    // min/max of -1..1 that maps lane 0 to 0.5 and lane 1 to +/-0.5 (the
    // vertical axis may be cartesian or screen-oriented; either way the
    // pointer's y must drive the second lane within the prop's bounds).
    let start = pad.center();
    let end = egui::pos2(
        pad.left() + pad.width() * 0.75,
        pad.top() + pad.height() * 0.25,
    );
    harness.hover_at(start);
    harness.run();
    harness.drag_at(start);
    harness.run();
    harness.hover_at(end);
    harness.run();
    // A raw release, as in `tween_curve_editor.rs`: `drop_at` also sends
    // PointerGone, which clears the pointer before the release is handled.
    harness.event(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();

    let actions = harness.state_mut().take_actions();
    let update = actions
        .iter()
        .find(|action| {
            action.action == "modules/updateProp" && action.payload["prop"] == "Position"
        })
        .unwrap_or_else(|| {
            panic!("dragging the vector pad must queue modules/updateProp for Position, got {actions:?}")
        });
    assert_eq!(update.payload["moduleId"], "ball-5678");
    let value = &update.payload["data"]["value"];
    let lanes = value.as_array().expect("the vector payload is an array");
    assert_eq!(lanes.len(), 2, "both vec2 lanes are written: {value}");
    close(lanes[0].as_f64(), 0.5, "x");
    let y = lanes[1].as_f64().expect("y lane is numeric");
    assert!(
        (y.abs() - 0.5).abs() <= 0.06,
        "the y lane must follow the pointer's vertical position within -1..1, got {y}"
    );

    // The numeric rows are still the accessible/keyboard path.
    assert!(
        harness.query_all_by_label("X").count() >= 1,
        "the numeric X lane remains accessible"
    );
    assert!(
        harness.query_all_by_label("Y").count() >= 1,
        "the numeric Y lane remains accessible"
    );
}
