//! Real-binary coverage for engine/status fixes and dispatch actions that had
//! no process-level test: published FPS metrics (18ae07e4), timing units
//! (f610d08b), Scene capture actions (bda55128), smoothing ids (72c49c1d) and
//! checkbox coercion on snapshot restore (fc8371c4).
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_engine_status -- --test-threads=1"
//! ```

#![cfg(unix)] // the control harness uses a Unix-domain socket

mod e2e;

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use e2e::VmodvHarness;

fn seed_scenes(user_data: &Path, scenes: Vec<Value>, performance: Value) {
    let document = json!({
        "version": 1,
        "defaultsSeeded": true,
        "defaultsVersion": 21,
        "seededDefaultScenes": [],
        "scenes": scenes,
        "performance": performance,
    });
    fs::write(
        user_data.join("scenes.json"),
        serde_json::to_string_pretty(&document).unwrap(),
    )
    .expect("write the seeded scenes.json");
}

fn group(id: &str, name: &str, modules: &[&str]) -> Value {
    json!({
        "id": id,
        "name": name,
        "hidden": false,
        "modules": modules,
        "enabled": 1,
        "alpha": 1,
        "inherit": false,
        "inheritFrom": -1,
        "pipeline": false,
        "clearing": true,
        "compositeOperation": "normal",
    })
}

fn module(id: &str, name: &str) -> Value {
    json!({
        "$id": id,
        "$moduleName": name,
        "meta": { "name": name, "enabled": true },
        "props": {},
        "data": {},
        "$props": {},
    })
}

fn scene(id: &str, name: &str, groups: Vec<Value>, modules: Value) -> Value {
    json!({
        "id": id,
        "name": name,
        "enabled": true,
        "groups": groups,
        "modules": modules,
        "inputs": {
            "focusedInput": { "id": null, "title": null },
            "inputs": {},
            "inputLinks": {},
        },
        "expressions": { "assignments": {} },
        "tweens": [],
        "midi": {},
    })
}

fn scene_with_id<'a>(state: &'a Value, scene_id: &str) -> Option<&'a Value> {
    state["scenes"]
        .as_array()?
        .iter()
        .find(|scene| scene["id"] == json!(scene_id))
}

fn live_cell_groups<'a>(state: &'a Value, scene_id: &str, row: usize) -> Vec<&'a Value> {
    state["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .filter(|group| group["sceneId"] == json!(scene_id) && group["sceneRow"] == json!(row))
        .collect()
}

fn read_scenes_file(user_data: &Path) -> Value {
    let path = user_data.join("scenes.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()))
}

fn no_transition(app: &mut VmodvHarness) {
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

/// 18ae07e4: the status bar used to show 0 FPS / 0x0 because the preview
/// metrics were never published. The socket snapshot must carry live values.
#[test]
fn runtime_status_reports_the_real_fps_and_resolution() {
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();
    app.wait_ready();
    let state = app.wait_for_state(Duration::from_secs(30), |state| {
        state["runtime"]["fps"].as_f64().unwrap_or(0.0) > 0.0
            && state["runtime"]["frameMs"].as_f64().unwrap_or(0.0) > 0.0
            && state["runtime"]["frames"].as_u64().unwrap_or(0) > 0
    });
    let resolution = state["runtime"]["resolution"]
        .as_array()
        .expect("the compositor resolution is published");
    assert!(
        resolution.len() == 2
            && resolution[0].as_u64().unwrap_or(0) > 0
            && resolution[1].as_u64().unwrap_or(0) > 0,
        "the compositor resolution is published: {state}"
    );
    assert_eq!(
        state["runtime"]["window"],
        json!([640, 360]),
        "the window size is published: {state}"
    );
    assert!(
        state["audio"]["sampleRate"].as_f64().unwrap_or(0.0) > 0.0,
        "the analyser exposes its sample rate: {state}"
    );
    assert!(
        state["audio"].get("captureRate").is_some(),
        "the real (or unavailable) audio capture rate is published: {state}"
    );

    app.quit();
    app.assert_exit(0);
}

/// f610d08b: switching the Scene timing unit converts the global config and
/// every per-Scene override, and the unit survives a restart.
#[test]
fn switching_the_timing_unit_converts_and_persists_it() {
    let root = e2e::temp_root("vmodv-e2e-timing-unit");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    let mut timed = scene(
        "scene-a",
        "Timed Scene",
        vec![group("a-group", "Background", &["a-ball"])],
        json!({ "a-ball": module("a-ball", "Ball") }),
    );
    timed["timingOverride"] = json!(true);
    timed["minSeconds"] = json!(1);
    timed["maxSeconds"] = json!(3);
    seed_scenes(
        &user_data,
        vec![timed],
        json!({
            "enabled": false,
            "timingUnit": "seconds",
            "minSeconds": 8,
            "maxSeconds": 24,
            "transitionSeconds": 2,
            "rowNames": [],
        }),
    );

    let mut first = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    first.wait_ready();
    first.dispatch("beats/SET_BPM", json!({ "bpm": 120 }));

    let config = first.dispatch("scenes/setTimingUnit", json!({ "unit": "bars" }));
    assert_eq!(config["timingUnit"], json!("bars"), "{config}");
    assert_eq!(
        config["minSeconds"],
        json!(4),
        "8 s at 120 BPM is 4 bars: {config}"
    );
    assert_eq!(config["maxSeconds"], json!(12), "{config}");
    let state = first.state();
    let converted = scene_with_id(&state, "scene-a").expect("the timed Scene");
    assert_eq!(converted["minSeconds"], json!(0.5), "{converted}");
    assert_eq!(converted["maxSeconds"], json!(1.5), "{converted}");
    assert_eq!(state["performance"]["timingUnit"], json!("bars"));

    first.quit();
    first.assert_exit(0);
    assert_eq!(
        read_scenes_file(&user_data)["performance"]["timingUnit"],
        json!("bars"),
        "the unit is saved to scenes.json"
    );

    let mut second = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    second.wait_ready();
    assert_eq!(
        second.state()["performance"]["timingUnit"],
        json!("bars"),
        "the unit is hydrated by the next process"
    );
    second.quit();
    second.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}

/// bda55128 / c06bfb1f: capturing the live stack into a Scene and saving the
/// current stack as a new Scene were dispatch wildcards before the rewrite.
#[test]
fn live_state_capture_and_save_current_as_scene_work_over_the_socket() {
    let root = e2e::temp_root("vmodv-e2e-scene-capture");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    seed_scenes(
        &user_data,
        vec![scene(
            "scene-a",
            "Capture Source",
            vec![group("a-group", "Background", &["a-ball"])],
            json!({ "a-ball": module("a-ball", "Ball") }),
        )],
        json!({ "enabled": false, "rowNames": [] }),
    );

    let mut app = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    app.wait_ready();
    no_transition(&mut app);
    app.dispatch("scenes/applyScene", json!({ "sceneId": "scene-a" }));
    let state = app.wait_for_state(Duration::from_secs(30), |state| {
        live_cell_groups(state, "scene-a", 0).len() == 1
    });
    let live_id = live_cell_groups(&state, "scene-a", 0)[0]["id"]
        .as_str()
        .expect("the live Group id")
        .to_string();

    let captured = app.dispatch(
        "scenes/captureLiveStateToScene",
        json!({ "sceneId": "scene-a" }),
    );
    assert!(
        captured.get("error").is_none(),
        "the live rows are captured: {captured}"
    );
    assert_eq!(
        captured["scene"]["groups"][0]["id"],
        json!(live_id),
        "the captured Scene holds the live Group: {captured}"
    );

    let saved = app.dispatch(
        "scenes/saveCurrentAsScene",
        json!({ "name": "E2E Snapshot" }),
    );
    let saved_id = saved["id"]
        .as_str()
        .expect("saveCurrentAsScene returns the new Scene")
        .to_string();
    assert_eq!(saved["name"], json!("E2E Snapshot"), "{saved}");
    let state = app.wait_for_state(Duration::from_secs(20), |state| {
        state["scenes"]
            .as_array()
            .is_some_and(|scenes| scenes.iter().any(|scene| scene["id"] == json!(saved_id)))
    });
    assert_eq!(
        scene_with_id(&state, &saved_id).expect("the saved Scene")["name"],
        json!("E2E Snapshot")
    );

    app.quit();
    app.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}

/// 72c49c1d: the string-payload smoothing-id actions bypass the object-only
/// dispatch view. A typo there would silently register nothing, so drive both
/// actions over the socket and check the minted ids are distinct.
#[test]
fn smoothing_id_actions_accept_their_string_payloads() {
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();
    app.wait_ready();

    let first = app.dispatch("meyda/getSmoothingId", json!({}));
    let second = app.dispatch("meyda/getSmoothingId", json!({}));
    let id = first.as_str().expect("getSmoothingId returns an id");
    assert!(
        !id.is_empty() && first != second,
        "each id is unique: {first}, {second}"
    );
    assert_eq!(
        app.dispatch("meyda/removeSmoothingId", json!(id)),
        Value::Null,
        "the string payload is accepted without an object wrapper"
    );

    app.quit();
    app.assert_exit(0);
}

/// fc8371c4 (restore path): saved snapshots written when the legacy UI
/// committed 1/0 for a Module's `enabled` meta must hydrate as real booleans.
#[test]
fn numeric_module_meta_in_a_saved_snapshot_hydrates_as_booleans() {
    let root = e2e::temp_root("vmodv-e2e-snapshot-flags");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    let snapshot = json!({
        "groups": [],
        "modules": {
            "m-on": {
                "$id": "m-on", "$moduleName": "Bands",
                "meta": { "name": "Bands", "enabled": 1 },
                "props": {}, "data": {}, "$props": {},
            },
            "m-off": {
                "$id": "m-off", "$moduleName": "Ball",
                "meta": { "name": "Ball", "enabled": 0 },
                "props": {}, "data": {}, "$props": {},
            },
        },
        "inputs": {},
        "expressions": {},
        "tweens": [],
        "midi": {},
    });
    fs::write(
        user_data.join("engine-state.json"),
        serde_json::to_string_pretty(&snapshot).unwrap(),
    )
    .expect("write the saved snapshot");

    let mut app = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    app.wait_ready();
    let state = app.wait_for_state(Duration::from_secs(20), |state| {
        state["modules"]["m-on"]["meta"]["enabled"].is_boolean()
            && state["modules"]["m-off"]["meta"]["enabled"].is_boolean()
    });
    assert_eq!(
        state["modules"]["m-on"]["meta"]["enabled"],
        json!(true),
        "{state}"
    );
    assert_eq!(
        state["modules"]["m-off"]["meta"]["enabled"],
        json!(false),
        "{state}"
    );

    app.quit();
    app.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}
