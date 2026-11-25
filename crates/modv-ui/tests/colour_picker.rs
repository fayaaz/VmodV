//! The Inspector's colour props must expose an accessible square/wheel picker
//! that writes the existing engine payload shapes without changing alpha.

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::model::{GROUP_ENABLED, InheritTarget};
use modv_ui::widgets::{
    ColorPicker, ColorPickerMode, hsv_to_rgb, rgb_to_hsv, square_position_to_rgb,
    wheel_position_to_rgb,
};
use modv_ui::{
    EngineSnapshot, GroupView, ModuleView, PropComponentView, PropView, UiAction, UiState,
};

fn assert_rgb_close(actual: [f32; 3], expected: [f32; 3], tolerance: f32, context: &str) {
    for (index, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{context}: channel {index} should be {expected}, got {actual} (rgb {actual:?})"
        );
    }
}

#[test]
fn hsv_to_rgb_primaries_edges_wrapping_and_clamping() {
    let cases = [
        (0.0, 1.0, 1.0, [1.0, 0.0, 0.0]),
        (120.0, 1.0, 1.0, [0.0, 1.0, 0.0]),
        (240.0, 1.0, 1.0, [0.0, 0.0, 1.0]),
        (360.0, 1.0, 1.0, [1.0, 0.0, 0.0]),
        (-120.0, 1.0, 1.0, [0.0, 0.0, 1.0]),
        (0.0, 0.0, 1.0, [1.0, 1.0, 1.0]),
        (0.0, 0.0, 0.0, [0.0, 0.0, 0.0]),
        (0.0, 2.0, -1.0, [0.0, 0.0, 0.0]),
        (0.0, -1.0, 0.5, [0.5, 0.5, 0.5]),
    ];
    for (hue, saturation, value, expected) in cases {
        assert_rgb_close(
            hsv_to_rgb(hue, saturation, value),
            expected,
            1e-3,
            &format!("hsv({hue}, {saturation}, {value})"),
        );
    }
}

#[test]
fn rgb_to_hsv_primaries_greys_and_round_trips() {
    let cases = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 1.0]),
        ([0.0, 1.0, 0.0], [120.0, 1.0, 1.0]),
        ([0.0, 0.0, 1.0], [240.0, 1.0, 1.0]),
        ([0.5, 0.5, 0.5], [0.0, 0.0, 0.5]),
    ];
    for (rgb, expected) in cases {
        let actual = rgb_to_hsv(rgb);
        for (index, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
            assert!(
                (actual - expected).abs() <= 1e-3,
                "rgb_to_hsv({rgb:?}): component {index} should be {expected}, got {actual:?}"
            );
        }
    }

    let colours = [
        [0.0, 0.7, 1.0],
        [45.0, 0.25, 0.8],
        [120.0, 1.0, 0.6],
        [275.0, 0.55, 0.9],
        [359.0, 0.9, 0.4],
    ];
    for hsv in colours {
        let rgb = hsv_to_rgb(hsv[0], hsv[1], hsv[2]);
        let round_tripped_rgb = {
            let [hue, saturation, value] = rgb_to_hsv(rgb);
            hsv_to_rgb(hue, saturation, value)
        };
        assert_rgb_close(
            round_tripped_rgb,
            rgb,
            1e-2,
            &format!("round trip for {hsv:?}"),
        );
    }
}

#[test]
fn square_position_to_rgb_maps_and_clamps_coordinates() {
    let cases = [
        (0.0, 1.0, [1.0, 1.0, 1.0]),
        (1.0, 1.0, [0.0, 1.0, 0.0]),
        (1.0, 0.0, [0.0, 0.0, 0.0]),
        (0.5, 1.0, [0.5, 1.0, 0.5]),
        (2.0, 2.0, [0.0, 1.0, 0.0]),
        (-1.0, -1.0, [0.0, 0.0, 0.0]),
    ];
    for (x, y, expected) in cases {
        assert_rgb_close(
            square_position_to_rgb(120.0, x, y),
            expected,
            1e-3,
            &format!("square(120 degrees, {x}, {y})"),
        );
    }
}

#[test]
fn wheel_position_to_rgb_maps_angles_and_clamps_radius() {
    let cases = [
        (0.0, 0.0, [1.0, 1.0, 1.0]),
        (1.0, 0.0, [1.0, 0.0, 0.0]),
        (0.0, 1.0, [0.5, 1.0, 0.0]),
        (-1.0, 0.0, [0.0, 1.0, 1.0]),
        (0.0, -1.0, [0.5, 0.0, 1.0]),
        (2.0, 0.0, [1.0, 0.0, 0.0]),
        (0.0, 0.5, [0.75, 1.0, 0.5]),
    ];
    for (x, y, expected) in cases {
        assert_rgb_close(
            wheel_position_to_rgb(x, y),
            expected,
            1e-3,
            &format!("wheel({x}, {y})"),
        );
    }
}

#[test]
fn color_picker_clamps_and_reports_only_actual_rgb_changes() {
    let mut picker = ColorPicker::new([-1.0, 2.0, 0.25, 1.5]);
    assert_eq!(picker.rgba, [0.0, 1.0, 0.25, 1.0]);
    assert_eq!(picker.mode, ColorPickerMode::Square);
    assert_eq!(
        picker.clone().with_mode(ColorPickerMode::Wheel).mode,
        ColorPickerMode::Wheel
    );

    assert!(picker.set_rgb([-0.5, 0.4, 2.0]));
    assert_eq!(picker.rgba, [0.0, 0.4, 1.0, 1.0]);
    assert!(!picker.set_rgb([0.0, 0.4, 1.0]));
    assert_eq!(picker.rgba, [0.0, 0.4, 1.0, 1.0]);
}

fn group(module_id: &str) -> GroupView {
    GroupView {
        id: "group-1".to_string(),
        name: "Colours".to_string(),
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

fn module(name: &str, id: &str, prop: PropView) -> ModuleView {
    ModuleView {
        id: id.to_string(),
        module_name: name.to_string(),
        group_id: "group-1".to_string(),
        enabled: true,
        alpha: 1.0,
        alpha_input_id: None,
        composite_operation: "normal".to_string(),
        composite_operation_input_id: None,
        module_type: "isf".to_string(),
        props: vec![prop],
    }
}

fn vec4_colour_prop() -> PropView {
    PropView {
        name: "keyColor".to_string(),
        prop_type: "vec4".to_string(),
        value: serde_json::json!([1.0, 0.0, 0.0, 1.0]),
        min: Some(0.0),
        max: Some(1.0),
        step: Some(0.001),
        default: None,
        input_id: Some("in-keycolor".to_string()),
        components: (0..4)
            .map(|index| PropComponentView {
                key: index.to_string(),
                input_id: format!("in-keycolor-{index}"),
            })
            .collect(),
        tween: None,
        enum_members: Vec::new(),
    }
}

fn color_prop() -> PropView {
    PropView {
        name: "fillColor".to_string(),
        prop_type: "color".to_string(),
        value: serde_json::json!({"r": 1.0, "g": 0.0, "b": 0.0, "a": 0.5}),
        min: None,
        max: None,
        step: None,
        default: None,
        input_id: Some("in-fill".to_string()),
        components: Vec::new(),
        tween: None,
        enum_members: Vec::new(),
    }
}

fn harness(module: ModuleView) -> Harness<'static, UiState> {
    let snapshot = EngineSnapshot {
        groups: vec![group(&module.id)],
        modules: vec![module],
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

fn select_module(harness: &mut Harness<'static, UiState>, name: &str, id: &str) {
    let short_id: String = id.chars().take(8).collect();
    let label = format!("{name}  ({short_id})");
    harness.get_by_label(&label).click();
    harness.run();
}

fn open_picker(harness: &mut Harness<'static, UiState>, prop: &str) {
    let label = format!("Colour picker for {prop}");
    harness.get_by_label(&label).click();
    // Popups are laid out after the opening click has been processed.
    harness.run();
    harness.run();
}

fn click_at(harness: &mut Harness<'static, UiState>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

fn updates_for<'a>(actions: &'a [UiAction], prop: &str) -> Vec<&'a UiAction> {
    actions
        .iter()
        .filter(|action| action.action == "modules/updateProp" && action.payload["prop"] == prop)
        .collect()
}

fn assert_ratio(actual: &serde_json::Value, expected: f64, context: &str) {
    let actual = actual.as_f64().unwrap_or(f64::NAN);
    assert!(
        (actual - expected).abs() <= 0.08,
        "{context} should be about {expected}, got {actual}"
    );
}

#[test]
fn vec4_colour_picker_updates_xyzw() {
    let mut harness = harness(module("Colour Key", "colour-key-1", vec4_colour_prop()));
    select_module(&mut harness, "Colour Key", "colour-key-1");
    open_picker(&mut harness, "keyColor");

    harness.get_by_label("Square");
    harness.get_by_label("Wheel");
    let square = harness.get_by_label("Colour square").rect();
    assert!(
        square.width() > 0.0 && square.height() > 0.0,
        "the colour square must have a drawable surface: {square:?}"
    );
    assert_eq!(
        harness.query_all_by_label("Vector pad").count(),
        0,
        "a four-lane colour control must not draw the vec2 XY pad"
    );
    for lane in ["X", "Y", "Z", "W"] {
        assert!(
            harness.query_all_by_label(lane).count() >= 1,
            "the numeric {lane} lane remains available"
        );
    }

    click_at(
        &mut harness,
        egui::pos2(square.center().x, square.top() + 2.0),
    );

    let actions = harness.state_mut().take_actions();
    let updates = updates_for(&actions, "keyColor");
    assert_eq!(
        updates.len(),
        1,
        "one square gesture must queue exactly one keyColor update: {actions:?}"
    );
    assert_eq!(updates[0].payload["moduleId"], "colour-key-1");
    let value = &updates[0].payload["data"]["value"];
    let lanes = value
        .as_array()
        .unwrap_or_else(|| panic!("vec4 updates keep the four-lane array shape: {value:?}"));
    assert_eq!(lanes.len(), 4, "all four vec4 lanes are written: {value:?}");
    assert_ratio(&lanes[0], 1.0, "square red lane");
    assert_ratio(&lanes[1], 0.5, "square green lane");
    assert_ratio(&lanes[2], 0.5, "square blue lane");
    assert_eq!(
        lanes[3].as_f64(),
        Some(1.0),
        "picking must preserve the W/alpha lane exactly: {value:?}"
    );

    harness.get_by_label("Wheel").click();
    harness.run();
    harness.run();
    harness.get_by_label("Colour wheel");
    assert_eq!(
        harness.query_all_by_label("Colour square").count(),
        0,
        "switching modes must persist across frames and replace the square with the wheel"
    );
    let wheel = harness.get_by_label("Colour wheel").rect();
    click_at(
        &mut harness,
        egui::pos2(wheel.right() - 2.0, wheel.center().y),
    );

    let actions = harness.state_mut().take_actions();
    let updates = updates_for(&actions, "keyColor");
    assert_eq!(
        updates.len(),
        1,
        "one wheel gesture must queue exactly one fresh keyColor update: {actions:?}"
    );
    let value = &updates[0].payload["data"]["value"];
    let lanes = value.as_array().expect("the wheel update is a vec4 array");
    assert_eq!(lanes.len(), 4);
    let red = lanes[0].as_f64().expect("red lane");
    let green = lanes[1].as_f64().expect("green lane");
    let blue = lanes[2].as_f64().expect("blue lane");
    assert!(
        red > 0.9 && green < 0.1 && blue < 0.1,
        "the wheel's right rim should select red, got {value:?}"
    );
    assert_eq!(
        lanes[3].as_f64(),
        Some(1.0),
        "the wheel must also preserve W/alpha exactly: {value:?}"
    );
}

#[test]
fn colour_prop_picker_updates_rgba_ratios() {
    let mut harness = harness(module("Tint", "tint-colour-1", color_prop()));
    select_module(&mut harness, "Tint", "tint-colour-1");
    open_picker(&mut harness, "fillColor");

    harness.get_by_label("Square");
    harness.get_by_label("Wheel");
    let square = harness.get_by_label("Colour square").rect();
    click_at(
        &mut harness,
        egui::pos2(square.center().x, square.top() + 2.0),
    );

    let actions = harness.state_mut().take_actions();
    let updates = updates_for(&actions, "fillColor");
    assert_eq!(
        updates.len(),
        1,
        "one colour gesture must queue exactly one fillColor update: {actions:?}"
    );
    assert_eq!(updates[0].payload["moduleId"], "tint-colour-1");
    let value = &updates[0].payload["data"]["value"];
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("color updates preserve the RGBA object shape: {value:?}"));
    assert_eq!(
        object.len(),
        4,
        "the colour payload has exactly r/g/b/a: {value:?}"
    );
    assert_ratio(&object["r"], 1.0, "colour red ratio");
    assert_ratio(&object["g"], 0.5, "colour green ratio");
    assert_ratio(&object["b"], 0.5, "colour blue ratio");
    assert_eq!(
        object["a"].as_f64(),
        Some(0.5),
        "picking must preserve the existing alpha ratio exactly: {value:?}"
    );
}
