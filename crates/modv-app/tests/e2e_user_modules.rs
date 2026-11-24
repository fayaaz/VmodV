//! Real-binary coverage for user-authored WGSL + JSON modules.
//!
//! Unit tests cover the descriptor parser, discovery and registry wiring; this
//! suite drives the built `modv` shell with a seeded `<userData>` tree so the
//! whole path — profile media folders → registry → `state.registered` → live
//! Module instance — is exercised without a mock.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_user_modules -- --test-threads=1"
//! ```

#![cfg(unix)] // the control harness uses a Unix-domain socket

mod e2e;

use std::fs;
use std::time::Duration;

use serde_json::{Value, json};

use e2e::VmodvHarness;

const USER_WGSL: &str = "@fragment fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> { return vec4<f32>(p.x / 100.0, p.y / 100.0, 0.0, 1.0); }";

fn write_module(dir: &std::path::Path, file: &str, descriptor: Value) {
    fs::write(
        dir.join(file),
        serde_json::to_string_pretty(&descriptor).unwrap(),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", dir.join(file).display()));
}

#[test]
fn user_modules_from_the_media_folder_are_registered_and_instantiable() {
    let root = e2e::temp_root("vmodv-e2e-user-modules");
    let user_data = root.join("userData");
    let module_dir = user_data.join("media/default/module");
    fs::create_dir_all(&module_dir).expect("create the profile module folder");

    // A sibling .wgsl is the documented "descriptor + source file" form.
    fs::write(module_dir.join("e2e_gradient.wgsl"), USER_WGSL).expect("write the WGSL source");
    write_module(
        &module_dir,
        "00_e2e_gradient.json",
        json!({
            "name": "E2E Gradient",
            "author": "E2E Author",
            "version": "2.3.4",
            "description": "a user module registered by the real shell",
            "category": "generative",
            "wgsl": "e2e_gradient.wgsl",
            "entry": "fs_main",
            "props": [{
                "name": "u_amount",
                "label": "Amount",
                "type": "float",
                "default": 0.5,
                "min": 0.0,
                "max": 1.0,
                "step": 0.01,
            }],
        }),
    );

    let mut app = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    app.wait_ready();

    let state = app.wait_for_state(Duration::from_secs(30), |state| {
        state["registered"]
            .get("E2E Gradient")
            .is_some_and(|entry| entry.is_object())
    });
    let entry = &state["registered"]["E2E Gradient"];
    assert_eq!(
        entry["meta"],
        json!({
            "name": "E2E Gradient",
            "author": "E2E Author",
            "version": "2.3.4",
            "type": "shader",
            "category": "generative",
            "previewWithOutput": true,
        }),
        "the user descriptor reaches the Gallery metadata: {entry}"
    );
    assert_eq!(entry["props"]["u_amount"]["type"], json!("float"));
    assert_eq!(entry["props"]["u_amount"]["default"], json!(0.5));
    assert_eq!(entry["props"]["u_amount"]["min"], json!(0.0));
    assert_eq!(entry["props"]["u_amount"]["max"], json!(1.0));

    // The registered definition behaves like a built-in one: it can be
    // instantiated through the same store action the Gallery uses, and the
    // compositor keeps presenting frames with it live.
    let module = app.dispatch(
        "modules/makeActiveModule",
        json!({ "moduleName": "E2E Gradient", "moduleMeta": { "enabled": true } }),
    );
    let module_id = module["$id"]
        .as_str()
        .expect("the user Module gets a live id")
        .to_string();
    let state = app.wait_for_state(Duration::from_secs(20), |state| {
        state["modules"].get(&module_id).is_some()
            && state["runtime"]["frames"].as_u64().unwrap_or(0) > 0
    });
    assert_eq!(
        state["modules"][&module_id]["$moduleName"],
        json!("E2E Gradient"),
        "the live Module uses the registered user definition: {module}"
    );
    assert_eq!(
        state["modules"][&module_id]["props"]["u_amount"],
        json!(0.5),
        "descriptor defaults seed the instance props"
    );

    let shot = app.temp_dir().join("user-module.png");
    let capture = app.screenshot(&shot, Some(320), Some(180));
    assert!(
        fs::metadata(&shot).is_ok_and(|meta| meta.len() > 100),
        "a frame with the user Module renders: {capture}"
    );

    app.quit();
    app.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn malformed_and_colliding_user_modules_are_skipped_at_startup() {
    let root = e2e::temp_root("vmodv-e2e-user-modules-skipped");
    let user_data = root.join("userData");
    // The flat (pre-profile) folder is still scanned, so this also proves the
    // legacy path remains wired.
    let module_dir = user_data.join("media/module");
    fs::create_dir_all(&module_dir).expect("create the flat module folder");

    write_module(
        &module_dir,
        "00_inline.json",
        json!({
            "name": "E2E Inline",
            "author": "Inline Author",
            "wgslSource": USER_WGSL,
        }),
    );
    fs::write(module_dir.join("01_broken.json"), "{ this is not json }")
        .expect("write a malformed descriptor");
    write_module(
        &module_dir,
        "02_collision.json",
        json!({
            "name": "Bands",
            "author": "E2E Collision Author",
            "wgslSource": USER_WGSL,
        }),
    );
    write_module(
        &module_dir,
        ".03_hidden.json",
        json!({
            "name": "E2E Hidden",
            "author": "Hidden Author",
            "wgslSource": USER_WGSL,
        }),
    );

    let mut app = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    app.wait_ready();

    let state = app.state();
    let registered = state["registered"]
        .as_object()
        .expect("registered is an object");
    assert!(
        registered.contains_key("E2E Inline"),
        "the valid inline descriptor registers: {state}"
    );
    for skipped in ["E2E Broken", "E2E Hidden", "E2E Collision Author"] {
        assert!(
            registered.keys().all(|name| name != skipped),
            "{skipped} must not be registered: {state}"
        );
    }
    assert_ne!(
        registered["Bands"]["meta"]["author"],
        json!("E2E Collision Author"),
        "a colliding name does not replace the built-in definition: {}",
        registered["Bands"]
    );
    assert!(
        registered["Bands"]["props"].get("u_ringWidth").is_some(),
        "the built-in Bands props survive the collision: {}",
        registered["Bands"]
    );

    app.quit();
    app.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}
