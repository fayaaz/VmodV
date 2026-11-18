//! Process-level native scenarios ported from the Electron e2e suite.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_process -- --test-threads=1"
//! ```
//!
//! Each test launches the real `modv` binary (the one Cargo just built) under
//! Xvfb when the environment has no display. Restart tests own a shared
//! `userData` tree via [`e2e::temp_root`], so state, Scenes and window
//! placement survive a process boundary exactly like an app relaunch.
//! Supervisor parity and the deliberate differences are described in
//! `docs/native-e2e.md`.

mod e2e;

use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use e2e::NativeHarness;

fn scene_document() -> Value {
    json!({
        "id": "scene-e2e-opened",
        "name": "E2E Opened Scene",
        "enabled": true,
        "groups": [{
            "id": "doc-opened-group",
            "name": "Opened Group",
            "hidden": false,
            "modules": ["doc-opened-ball"],
            "enabled": 1,
            "alpha": 1,
            "inherit": false,
            "inheritFrom": -1,
            "pipeline": false,
            "clearing": true,
            "compositeOperation": "normal",
        }],
        "modules": {
            "doc-opened-ball": {
                "$id": "doc-opened-ball",
                "$moduleName": "Ball",
                "meta": { "name": "Ball", "enabled": true },
                "props": { "amount": 3 },
                "data": {},
                "$props": {},
            },
        },
    })
}

fn group_names(state: &Value) -> Vec<&str> {
    state["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .filter_map(|group| group["name"].as_str())
        .collect()
}

fn png_bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("screenshot {}: {error}", path.display()))
}

/// `engine/engineSkeleton.spec.js`: frames render and a killed engine is
/// relaunched over the same userData from its last snapshot.
///
/// The Rust supervisor has no test seam for a real `MODV_ENGINE` child, so the
/// native single-process shell stands in: it saves the same snapshot the
/// supervisor restores and proves the killed process can be replaced without a
/// stale state/lock.
#[test]
fn engine_skeleton_survives_sigkill_and_restores_state() {
    let root = e2e::temp_root("native-process-skeleton");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");

    let mut first = NativeHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    let pong = first.wait_ready();
    let first_pid = first.pid();
    assert_eq!(pong["pid"], json!(first_pid), "{pong}");

    first.dispatch(
        "groups/createGroup",
        json!({ "id": "crash-survivor", "name": "Crash Survivor" }),
    );
    first.wait_for_state(Duration::from_secs(20), |state| {
        state["runtime"]["frames"].as_u64().unwrap_or(0) > 5
            && group_names(state).contains(&"Crash Survivor")
    });

    let frame_path = first.temp_dir().join("before-kill.png");
    let screenshot = first.screenshot(&frame_path, Some(320), Some(180));
    assert_eq!(screenshot["width"], json!(320), "{screenshot}");
    assert_eq!(screenshot["height"], json!(180), "{screenshot}");
    assert!(
        png_bytes(&frame_path).len() > 100,
        "the engine presented a frame before the kill"
    );

    // The supervisor restores the last snapshot, not in-memory state: write
    // one now, then die without the graceful quit path.
    let saved = first.save();
    assert_eq!(
        saved["path"],
        json!(first.state_path().to_string_lossy()),
        "{saved}"
    );
    assert!(first.state_path().is_file(), "{saved}");

    let status = first.kill_and_wait();
    assert_eq!(status.signal(), Some(9), "SIGKILL expected: {status}");
    assert_eq!(status.code(), None, "SIGKILL expected: {status}");

    let mut second = NativeHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    let pong = second.wait_ready();
    assert_ne!(
        pong["pid"],
        json!(first_pid),
        "the replacement process is a new shell"
    );

    let state = second.state();
    assert!(
        state["groups"]
            .as_array()
            .expect("groups array")
            .iter()
            .any(|group| group["id"] == json!("crash-survivor")),
        "the saved snapshot is restored after the kill: {state}"
    );
    assert!(
        state["runtime"]["frames"].as_u64().unwrap_or(0) > 0,
        "the relaunched shell renders: {state}"
    );

    second.quit();
    second.assert_exit(0);

    let _ = fs::remove_dir_all(root);
}

/// `general/rendersTheMainWindow.spec.js`: the main window becomes ready and
/// the compositor presents a frame at the requested 16:9 size.
#[test]
fn renders_the_main_window_at_the_requested_size() {
    let mut app = NativeHarness::builder().width(640).height(360).spawn();
    app.wait_ready();
    let state = app.wait_for_state(Duration::from_secs(20), |state| {
        state["runtime"]["frames"].as_u64().unwrap_or(0) > 0
    });
    assert_eq!(
        state["runtime"]["window"],
        json!([640, 360]),
        "the main window uses the requested logical size: {state}"
    );

    let path = app.temp_dir().join("main-window.png");
    let screenshot = app.screenshot(&path, Some(640), Some(360));
    assert_eq!(screenshot["width"], json!(640), "{screenshot}");
    assert_eq!(screenshot["height"], json!(360), "{screenshot}");

    let bytes = png_bytes(&path);
    assert!(bytes.len() > 100, "PNG has content ({} bytes)", bytes.len());
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");
    let image = image::open(&path).expect("decode screenshot").to_rgba8();
    assert_eq!(image.dimensions(), (640, 360));
    assert_eq!(
        u64::from(image.width()) * 9,
        u64::from(image.height()) * 16,
        "16:9 output"
    );

    app.quit();
    app.assert_exit(0);
}

/// `groups/createsADefaultGroup.spec.js`: a fresh shell has one enabled user
/// Group, and relaunching over its snapshot does not create another.
#[test]
fn fresh_native_start_creates_exactly_one_default_group() {
    let root = e2e::temp_root("native-process-default-group");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");

    let mut first = NativeHarness::builder().user_data(&user_data).spawn();
    first.wait_ready();
    let state = first.state();
    assert_eq!(
        group_names(&state),
        ["New Group"],
        "a fresh shell creates Electron's default Group: {state}"
    );
    let default_id = state["groups"][0]["id"]
        .as_str()
        .expect("default Group id")
        .to_string();
    assert_eq!(state["groups"][0]["enabled"], json!(1), "{state}");

    first.dispatch(
        "groups/createGroup",
        json!({ "id": "user-group", "name": "User Group" }),
    );
    first.quit();
    first.assert_exit(0);

    // The snapshot now holds two Groups; a second boot must restore both and
    // not stack a second "New Group" on top.
    let mut second = NativeHarness::builder().user_data(&user_data).spawn();
    second.wait_ready();
    let state = second.state();
    let groups = state["groups"].as_array().expect("groups array");
    assert_eq!(
        groups.len(),
        2,
        "restored Groups are not duplicated: {state}"
    );
    assert_eq!(
        groups
            .iter()
            .filter(|group| group["name"] == json!("New Group"))
            .count(),
        1,
        "{state}"
    );
    assert_eq!(
        groups
            .iter()
            .filter(|group| group["id"] == json!(default_id))
            .count(),
        1,
        "the default Group keeps its restored id: {state}"
    );
    assert!(
        group_names(&state).contains(&"User Group"),
        "the user Group is restored: {state}"
    );

    second.quit();
    second.assert_exit(0);

    let _ = fs::remove_dir_all(root);
}

/// `main/windowState.spec.js`: the main/output placement is written on quit
/// and honoured on the next launch, even when new CLI dimensions are passed.
#[test]
fn window_state_survives_a_restart() {
    let root = e2e::temp_root("native-process-window-state");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");

    let mut first = NativeHarness::builder()
        .width(720)
        .height(405)
        .user_data(&user_data)
        .spawn();
    first.wait_ready();
    first.wait_for_state(Duration::from_secs(20), |state| {
        state["runtime"]["window"] == json!([720, 405])
            && state["runtime"]["frames"].as_u64().unwrap_or(0) > 0
    });
    first.quit();
    first.assert_exit(0);

    let window_state_path = user_data.join("window-state.json");
    let text = fs::read_to_string(&window_state_path).unwrap_or_else(|error| {
        panic!(
            "quit must write {}: {error}\n--- logs ---\n{}",
            window_state_path.display(),
            first.logs()
        )
    });
    let saved: Value = serde_json::from_str(&text).expect("window-state.json is JSON");
    assert_eq!(saved["version"], json!(1), "{saved}");
    assert_eq!(saved["mainWindow"]["width"], json!(720.0), "{saved}");
    assert_eq!(saved["mainWindow"]["height"], json!(405.0), "{saved}");
    assert_eq!(saved["mainWindow"]["maximized"], json!(false), "{saved}");
    let saved_output = saved["windows"]["output-0"]
        .as_object()
        .unwrap_or_else(|| panic!("the output window placement is saved: {saved}"));
    let output_width = saved_output["width"]
        .as_f64()
        .expect("output width")
        .round() as u64;
    let output_height = saved_output["height"]
        .as_f64()
        .expect("output height")
        .round() as u64;
    assert!(output_width > 0 && output_height > 0, "{saved}");

    // Different requested dimensions must lose to the saved placement; the
    // output window restores its saved size too.
    let mut second = NativeHarness::builder()
        .width(800)
        .height(600)
        .user_data(&user_data)
        .spawn();
    second.wait_ready();
    let state = second.wait_for_state(Duration::from_secs(20), |state| {
        state["runtime"]["window"] == json!([720, 405])
    });
    let resolution = state["runtime"]["resolution"]
        .as_array()
        .expect("resolution array");
    assert_eq!(resolution[0].as_u64(), Some(output_width), "{state}");
    assert_eq!(resolution[1].as_u64(), Some(output_height), "{state}");

    second.quit();
    second.assert_exit(0);

    let _ = fs::remove_dir_all(root);
}

/// `scenes/sceneSnapshotsAndPerformance.spec.js`: opening a Scene JSON like a
/// preset lists the Scene **and applies it**, so its Groups/Modules are live
/// (the JS `loadSceneFromJson` default).
#[test]
fn a_scene_json_opened_like_a_preset_is_applied_as_a_scene() {
    let root = e2e::temp_root("native-process-scene-open");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    let scene_path = root.join("opened-scene.json");
    fs::write(
        &scene_path,
        serde_json::to_vec(&scene_document()).expect("scene JSON"),
    )
    .expect("write the Scene document");

    let mut app = NativeHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .preset(&scene_path)
        .spawn();
    app.wait_ready();

    let state = app.wait_for_state(Duration::from_secs(20), |state| {
        state["appliedSceneId"] == json!("scene-e2e-opened")
    });
    assert!(
        state["scenes"]
            .as_array()
            .expect("scenes array")
            .iter()
            .any(|scene| scene["id"] == json!("scene-e2e-opened")),
        "the opened Scene is in the authoritative list: {state}"
    );
    assert_eq!(
        state["performance"]["currentSceneId"],
        json!("scene-e2e-opened"),
        "{state}"
    );
    assert_eq!(
        group_names(&state),
        ["Opened Group"],
        "the Scene's Groups are live and no default Group is seeded: {state}"
    );
    let groups = state["groups"].as_array().expect("groups array");
    let module_ids = groups[0]["modules"].as_array().expect("group modules");
    assert_eq!(module_ids.len(), 1, "{state}");
    let module_id = module_ids[0].as_str().expect("module id");
    assert_eq!(
        state["modules"][module_id]["$moduleName"],
        json!("Ball"),
        "the Scene's Module is instantiated: {state}"
    );

    app.quit();
    app.assert_exit(0);

    let _ = fs::remove_dir_all(root);
}
