//! Process-level e2e tests for the VmodV shell's control socket.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_harness -- --test-threads=1"
//! ```
//!
//! Each test launches the real `modv` binary (the one Cargo just built) under
//! Xvfb when the environment has no display. See `tests/e2e/mod.rs` for the
//! harness and `docs/vmodv-e2e.md` for the protocol/mapping notes.

#![cfg(unix)] // the control harness uses Unix-domain sockets

mod e2e;

use serde_json::{Value, json};

use e2e::VmodvHarness;

fn scene_with_groups(state: &Value) -> Value {
    state["scenes"]
        .as_array()
        .expect("scenes array")
        .iter()
        .find(|scene| {
            scene
                .get("groups")
                .and_then(Value::as_array)
                .is_some_and(|groups| !groups.is_empty())
        })
        .cloned()
        .unwrap_or_else(|| panic!("no default Scene with Groups in {state}"))
}

#[test]
fn vmodv_shell_serves_state_seeds_defaults_and_quits_cleanly() {
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();

    let pong = app.wait_ready();
    assert_eq!(pong["pong"], json!(true), "{pong}");
    assert_eq!(pong["pid"], json!(app.pid()));

    let version = app.version();
    assert_eq!(version["binary"], json!("modv"), "{version}");
    assert_eq!(version["protocol"], json!(1), "{version}");
    assert!(version["version"].is_string(), "{version}");

    let state = app.state();
    let groups = state["groups"].as_array().expect("groups array");
    assert_eq!(
        groups.len(),
        1,
        "a fresh userData starts with Electron's default Group: {state}"
    );
    assert_eq!(groups[0]["name"], json!("New Group"), "{state}");
    assert_eq!(groups[0]["enabled"], json!(1), "{state}");
    let scenes = state["scenes"].as_array().expect("scenes array");
    assert!(
        !scenes.is_empty(),
        "default Scenes are seeded before readiness: {state}"
    );
    assert!(
        scenes.iter().all(|scene| scene.get("id").is_some()),
        "every seeded Scene has an id: {state}"
    );
    assert_eq!(state["defaultsSeeded"], json!(true), "{state}");
    assert!(state["audio"].is_object(), "audio summary: {state}");
    assert!(state["tweens"].is_array(), "tweens are part of state");
    assert!(state["inputs"].is_object(), "inputs are part of state");
    assert!(state["performance"].is_object(), "{state}");
    assert!(
        state["_engine"]["framesPresented"].is_u64(),
        "the engine sidecar mirrors engine-state.json: {state}"
    );
    assert_eq!(state["runtime"]["window"], json!([640, 360]));
    assert_eq!(
        state["runtime"]["socket"],
        json!(app.socket_path().to_string_lossy())
    );

    let quit = app.quit();
    assert_eq!(quit["quitting"], json!(true), "{quit}");
    app.assert_exit(0);
    assert!(
        !app.socket_path().exists(),
        "a graceful quit unlinks the control socket"
    );

    let snapshot_path = app.state_path().to_path_buf();
    let snapshot = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|error| {
        panic!(
            "quit must write {}: {error}\n--- logs ---\n{}",
            snapshot_path.display(),
            app.logs()
        )
    });
    let snapshot: Value = serde_json::from_str(&snapshot).expect("engine-state.json is JSON");
    assert!(snapshot["groups"].is_array(), "{snapshot}");
    assert!(snapshot["modules"].is_object(), "{snapshot}");
    assert!(snapshot["_engine"].is_object(), "{snapshot}");
}

#[test]
fn dispatch_updates_state_and_screenshot_writes_a_png() {
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();
    app.wait_ready();

    let group = app.dispatch("groups/createGroup", json!({ "name": "E2E" }));
    assert_eq!(group["name"], json!("E2E"), "{group}");
    let state = app.state();
    assert!(
        state["groups"]
            .as_array()
            .expect("groups array")
            .iter()
            .any(|candidate| candidate["name"] == json!("E2E")),
        "groups/createGroup is reflected in state: {state}"
    );

    let scene = scene_with_groups(&state);
    let scene_id = scene["id"].as_str().expect("Scene id").to_string();
    app.dispatch("scenes/applyScene", json!({ "sceneId": scene_id }));
    let state = app.state();
    assert_eq!(state["appliedSceneId"], json!(scene_id), "{state}");
    assert!(
        !state["groups"].as_array().expect("groups array").is_empty(),
        "applying a Scene builds its live Groups: {state}"
    );
    assert!(
        state["runtime"]["frames"].as_u64().unwrap_or(0) > 0,
        "the render loop advanced: {state}"
    );

    let png_path = app.temp_dir().join("frame.png");
    let screenshot = app.screenshot(&png_path, Some(320), Some(180));
    assert_eq!(screenshot["width"], json!(320), "{screenshot}");
    assert_eq!(screenshot["height"], json!(180), "{screenshot}");
    assert_eq!(screenshot["path"], json!(png_path.to_string_lossy()));

    let bytes = std::fs::read(&png_path).unwrap_or_else(|error| {
        panic!(
            "screenshot must write {}: {error}\n--- logs ---\n{}",
            png_path.display(),
            app.logs()
        )
    });
    assert!(bytes.len() > 100, "PNG has content ({} bytes)", bytes.len());
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");
    assert_eq!(
        screenshot["bytes"].as_u64(),
        Some(bytes.len() as u64),
        "{screenshot}"
    );
    let image = image::open(&png_path)
        .expect("decode screenshot")
        .to_rgba8();
    assert_eq!(image.dimensions(), (320, 180));

    app.quit();
    app.assert_exit(0);
}

#[test]
fn control_socket_reports_errors_reconnects_and_logs() {
    let mut app = VmodvHarness::spawn();
    app.wait_ready();

    let error = app
        .call(json!({ "cmd": "not-a-command" }))
        .expect_err("unknown commands fail");
    assert!(error.contains("unknown cmd"), "{error}");

    let error = app
        .try_dispatch("modules/updateProp", json!({}))
        .expect_err("missing fields fail");
    assert!(error.contains("moduleId"), "{error}");

    // A dropped connection must not take the server down.
    app.disconnect();
    let pong = app.ping();
    assert_eq!(pong["pong"], json!(true), "{pong}");

    let logs = app.logs();
    assert!(
        logs.contains("control socket listening"),
        "the shell announces the socket:\n{logs}"
    );

    app.quit();
    app.assert_exit(0);
}
