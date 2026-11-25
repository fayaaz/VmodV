//! Real-binary coverage for the colour picker's engine/render contract and
//! persistence.
//!
//! Coverage boundary: the picker's square/wheel pointer gesture and popup are
//! covered by the kittest suite in `crates/modv-ui/tests/colour_picker.rs`.
//! This file drives the built `modv` binary to check that the picker's JSON
//! payload shapes reach the live store, change the rendered frame, and survive
//! an `engine-state.json` save/restart.
//!
//! Screenshot assertions decode a PNG flattened over opaque black (the
//! presented frame is black-backed, not a transparent overlay). `screenshot`
//! reads the compositor's main target before present, so the sRGB present
//! decode is guarded separately by the compositor unit test
//! `present_srgb_target_shows_the_same_values_as_the_live_preview`.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_colour -- --test-threads=1"
//! ```

#![cfg(unix)] // the control harness uses a Unix-domain socket

mod e2e;

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use e2e::VmodvHarness;

fn disable_performance(app: &mut VmodvHarness) {
    app.dispatch(
        "scenes/updatePerformance",
        json!({ "config": {
            "enabled": false,
            "allowedTransitions": ["None"],
            "transitionSeconds": 0.1,
            "order": "in-order",
        }}),
    );
}

fn create_group(app: &mut VmodvHarness, name: &str) -> String {
    let group = app.dispatch(
        "groups/createGroup",
        json!({
            "name": name,
            "enabled": 1,
            "alpha": 1.0,
            "clearing": true,
            "inherit": false,
            "pipeline": false,
            "compositeOperation": "normal",
        }),
    );
    group["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created Group has an id: {group}"))
        .to_string()
}

fn make_module(app: &mut VmodvHarness, name: &str, composite_operation: &str) -> String {
    let module = app.dispatch(
        "modules/makeActiveModule",
        json!({
            "moduleName": name,
            "moduleMeta": {
                "enabled": true,
                "alpha": 1.0,
                "compositeOperation": composite_operation,
            },
        }),
    );
    module["$id"]
        .as_str()
        .unwrap_or_else(|| panic!("created Module has an id: {module}"))
        .to_string()
}

fn add_module(app: &mut VmodvHarness, group_id: &str, module_id: &str) {
    app.dispatch(
        "groups/ADD_MODULE_TO_GROUP",
        json!({ "groupId": group_id, "moduleId": module_id }),
    );
}

fn update_prop(app: &mut VmodvHarness, module_id: &str, prop: &str, value: Value) {
    app.dispatch(
        "modules/updateProp",
        json!({
            "moduleId": module_id,
            "prop": prop,
            "data": { "value": value },
        }),
    );
}

fn wait_for_next_frame(app: &mut VmodvHarness, previous_frame: u64) -> Value {
    app.wait_for_state(Duration::from_secs(20), |state| {
        state["runtime"]["frames"].as_u64().unwrap_or(0) > previous_frame
    })
}

/// Wait until every Module is live inside the Group, optionally past a frame.
fn wait_for_group_modules(
    app: &mut VmodvHarness,
    group_id: &str,
    module_ids: &[&str],
    after_frame: Option<u64>,
) -> Value {
    app.wait_for_state(Duration::from_secs(30), |state| {
        module_ids
            .iter()
            .all(|id| state["modules"].get(*id).is_some_and(Value::is_object))
            && state["groups"].as_array().is_some_and(|groups| {
                groups.iter().any(|group| {
                    group["id"] == json!(group_id) && group["modules"] == json!(module_ids)
                })
            })
            && after_frame
                .is_none_or(|frame| state["runtime"]["frames"].as_u64().unwrap_or(0) > frame)
    })
}

fn assert_pixel_close(actual: [u8; 4], expected: [u8; 4], tolerance: u8, label: &str) {
    for channel in 0..4 {
        let difference = i16::from(actual[channel].abs_diff(expected[channel]));
        assert!(
            difference <= i16::from(tolerance),
            "{label}: expected {expected:?} within +/-{tolerance}, got {actual:?}"
        );
    }
}

fn centre_pixel(path: &Path) -> [u8; 4] {
    let image = image::open(path)
        .unwrap_or_else(|error| panic!("decode screenshot {}: {error}", path.display()))
        .to_rgba8();
    assert_eq!(image.dimensions(), (64, 64), "{}", path.display());
    image.get_pixel(32, 32).0
}

fn assert_color_object(value: &Value, expected: Value, label: &str) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{label} must be a colour object: {value}"));
    assert_eq!(
        object.len(),
        4,
        "{label} must contain exactly r/g/b/a: {value}"
    );
    assert_eq!(value, &expected, "{label}: {value}");
}

fn assert_vec4(value: &Value, expected: Value, label: &str) {
    let array = value
        .as_array()
        .unwrap_or_else(|| panic!("{label} must be a vec4 array: {value}"));
    assert_eq!(array.len(), 4, "{label} must contain four values: {value}");
    assert_eq!(value, &expected, "{label}: {value}");
}

#[test]
fn colour_key_chroma_keys_a_live_solid_colour_through_colour_prop_updates() {
    let mut app = VmodvHarness::builder().width(320).height(180).spawn();
    app.wait_ready();
    disable_performance(&mut app);

    let initial_state = app.state();
    let initial_frame = initial_state["runtime"]["frames"]
        .as_u64()
        .expect("runtime frame counter");
    let group_id = create_group(&mut app, "Colour Key E2E");
    let solid_id = make_module(&mut app, "Solid Color", "normal");
    let key_id = make_module(&mut app, "Colour Key", "copy");
    add_module(&mut app, &group_id, &solid_id);
    add_module(&mut app, &group_id, &key_id);

    let state = wait_for_group_modules(
        &mut app,
        &group_id,
        &[solid_id.as_str(), key_id.as_str()],
        Some(initial_frame),
    );

    assert_eq!(
        state["registered"]["Solid Color"]["props"]["Color"]["default"],
        json!([1, 0, 0, 1]),
        "Solid Color's registered default is opaque red: {state}"
    );
    assert_eq!(
        state["registered"]["Colour Key"]["props"]["keyColor"]["default"],
        json!([0, 1, 0, 1]),
        "Colour Key's registered default key is opaque green: {state}"
    );

    let red_path = app.temp_dir().join("colour-key-red.png");
    app.screenshot(&red_path, Some(64), Some(64));
    assert_pixel_close(
        centre_pixel(&red_path),
        [255, 0, 0, 255],
        2,
        "opaque red Solid Color passes the default green key",
    );

    let before_update = state["runtime"]["frames"]
        .as_u64()
        .expect("runtime frame counter");
    update_prop(&mut app, &key_id, "keyColor", json!([1.0, 0.0, 0.0, 1.0]));
    let state = wait_for_next_frame(&mut app, before_update);
    assert_vec4(
        &state["modules"][&key_id]["props"]["keyColor"],
        json!([1.0, 0.0, 0.0, 1.0]),
        "the vec4 colour-picker payload updates keyColor live",
    );

    let black_path = app.temp_dir().join("colour-key-black.png");
    app.screenshot(&black_path, Some(64), Some(64));
    assert_pixel_close(
        centre_pixel(&black_path),
        [0, 0, 0, 255],
        2,
        "keying out red leaves the opaque present backdrop black",
    );
    assert_eq!(
        state["modules"][&key_id]["meta"]["compositeOperation"],
        json!("copy"),
        "the transparent Colour Key result replaces the Group canvas: {state}"
    );

    let before_restore = state["runtime"]["frames"]
        .as_u64()
        .expect("runtime frame counter");
    update_prop(&mut app, &key_id, "keyColor", json!([0.0, 1.0, 0.0, 1.0]));
    let state = wait_for_next_frame(&mut app, before_restore);
    assert_vec4(
        &state["modules"][&key_id]["props"]["keyColor"],
        json!([0.0, 1.0, 0.0, 1.0]),
        "keyColor can be changed back live",
    );

    let restored_path = app.temp_dir().join("colour-key-restored.png");
    app.screenshot(&restored_path, Some(64), Some(64));
    assert_pixel_close(
        centre_pixel(&restored_path),
        [255, 0, 0, 255],
        2,
        "restoring the green key presents the red Solid Color again",
    );

    app.quit();
    app.assert_exit(0);
}

#[test]
fn colour_prop_values_keep_their_picker_shapes_across_a_restart() {
    let root = e2e::temp_root("vmodv-e2e-colour-persist");
    let user_data = root.join("userData");
    let color1 = json!({ "r": 0.25, "g": 0.5, "b": 0.75, "a": 1.0 });
    let red = json!([1.0, 0.0, 0.0, 1.0]);

    let mut first = VmodvHarness::builder()
        .width(320)
        .height(180)
        .user_data(&user_data)
        .spawn();
    first.wait_ready();
    disable_performance(&mut first);

    let group_id = create_group(&mut first, "Colour Persistence E2E");
    let bar_id = make_module(&mut first, "Bar", "normal");
    let key_id = make_module(&mut first, "Colour Key", "copy");
    add_module(&mut first, &group_id, &bar_id);
    add_module(&mut first, &group_id, &key_id);
    wait_for_group_modules(
        &mut first,
        &group_id,
        &[bar_id.as_str(), key_id.as_str()],
        None,
    );

    update_prop(&mut first, &bar_id, "color1", color1.clone());
    update_prop(&mut first, &key_id, "keyColor", red.clone());
    let state = first.state();
    assert_color_object(
        &state["modules"][&bar_id]["props"]["color1"],
        color1.clone(),
        "the color picker's object payload remains an object",
    );
    assert_vec4(
        &state["modules"][&key_id]["props"]["keyColor"],
        red.clone(),
        "the vec4 colour picker's payload remains an array",
    );

    first.save();
    let saved: Value = serde_json::from_str(
        &fs::read_to_string(first.state_path())
            .unwrap_or_else(|error| panic!("read {}: {error}", first.state_path().display())),
    )
    .unwrap_or_else(|error| panic!("parse {}: {error}", first.state_path().display()));
    assert_color_object(
        &saved["modules"][&bar_id]["props"]["color1"],
        color1.clone(),
        "the saved snapshot keeps color1 as an object",
    );
    assert_vec4(
        &saved["modules"][&key_id]["props"]["keyColor"],
        red.clone(),
        "the saved snapshot keeps keyColor as an array",
    );

    first.quit();
    first.assert_exit(0);

    let mut second = VmodvHarness::builder()
        .width(320)
        .height(180)
        .user_data(&user_data)
        .spawn();
    second.wait_ready();
    let state = second.wait_for_state(Duration::from_secs(30), |state| {
        state["modules"].get(&bar_id).is_some_and(Value::is_object)
            && state["modules"].get(&key_id).is_some_and(Value::is_object)
    });
    assert_color_object(
        &state["modules"][&bar_id]["props"]["color1"],
        color1,
        "the restart restores color1 as the original object",
    );
    assert_vec4(
        &state["modules"][&key_id]["props"]["keyColor"],
        red,
        "the restart restores keyColor as the original array",
    );

    second.quit();
    second.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}
