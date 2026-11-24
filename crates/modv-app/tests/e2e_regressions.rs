//! Real-binary regression coverage for fixes from the Rust rewrite.
//!
//! The fixes themselves are covered by unit/kittest tests; these scenarios
//! drive the built `modv` shell over its JSON-lines control socket so the same
//! behaviour is checked through the authoritative `state` snapshot, the
//! dispatch actions a VJ actually uses and the files left in `<userData>`.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_regressions -- --test-threads=1"
//! ```

#![cfg(unix)] // the control harness uses a Unix-domain socket

mod e2e;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use e2e::VmodvHarness;

/// Seed `<userData>/scenes.json` so the shell boots with known Scenes instead
/// of the defaults. `defaultsSeeded` + the current `defaultsVersion` stop the
/// seed pass adding its own Scenes over the fixture.
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

fn scene_with_id_owned(state: &Value, scene_id: &str) -> Value {
    scene_with_id(state, scene_id)
        .unwrap_or_else(|| panic!("Scene {scene_id} is missing: {state}"))
        .clone()
}

fn live_cell_groups<'a>(state: &'a Value, scene_id: &str, row: usize) -> Vec<&'a Value> {
    state["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .filter(|group| group["sceneId"] == json!(scene_id) && group["sceneRow"] == json!(row))
        .collect()
}

fn live_module_names(state: &Value, group: &Value) -> Vec<String> {
    group["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|id| {
            state["modules"][id]["$moduleName"]
                .as_str()
                .unwrap_or("<not instantiated>")
                .to_string()
        })
        .collect()
}

fn cell_module_names(scene_doc: &Value, row: usize) -> Vec<String> {
    let cell = scene_doc["groups"][row].clone();
    cell["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|id| {
            scene_doc["modules"][id]["$moduleName"]
                .as_str()
                .unwrap_or("<no snapshot>")
                .to_string()
        })
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

/// f8242f02: opening a Scene document used to require an explicit `apply`.
#[test]
fn loading_a_scene_without_an_apply_flag_applies_it() {
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();
    app.wait_ready();
    no_transition(&mut app);

    let document = scene(
        "e2e-implicit-apply",
        "E2E Implicit Apply",
        vec![group("doc-group", "Foreground", &["doc-ball"])],
        json!({ "doc-ball": module("doc-ball", "Ball") }),
    );
    let loaded = app.dispatch("scenes/loadSceneFromJson", json!({ "scene": document }));
    assert_eq!(loaded["id"], json!("e2e-implicit-apply"), "{loaded}");

    let state = app.wait_for_state(Duration::from_secs(30), |state| {
        state["appliedSceneId"] == json!("e2e-implicit-apply")
            && live_cell_groups(state, "e2e-implicit-apply", 0).len() == 1
    });
    assert_eq!(
        state["performance"]["currentSceneId"],
        json!("e2e-implicit-apply"),
        "the implicit apply updates the performance stamp: {state}"
    );
    let live = &live_cell_groups(&state, "e2e-implicit-apply", 0)[0];
    assert_eq!(
        live_module_names(&state, live),
        ["Ball"],
        "the applied Scene's Module is live: {live}"
    );

    app.quit();
    app.assert_exit(0);
}

/// bbe8e882 / 69cb78da: a whole-Scene apply replaces MIDI state and remaps the
/// focused input onto the rebuilt Module input id (no merge).
#[test]
fn applying_a_scene_restores_midi_and_the_focused_input_over_the_socket() {
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();
    app.wait_ready();
    no_transition(&mut app);

    app.dispatch(
        "midi/ADD_DEVICE",
        json!({
            "id": "live-ctrl", "name": "Live", "manufacturer": "Acme",
        }),
    );
    app.dispatch(
        "inputs/setFocusedInput",
        json!({
            "id": "live-input", "title": "Live",
        }),
    );
    let state = app.state();
    assert!(
        state["midi"]["devices"]
            .get("live-ctrl-Live-Acme")
            .is_some(),
        "the live MIDI device exists before the apply: {state}"
    );

    let mut document = scene(
        "e2e-midi-scene",
        "E2E MIDI Scene",
        vec![group("doc-group", "Foreground", &["doc-ball"])],
        json!({
            "doc-ball": {
                "$id": "doc-ball",
                "$moduleName": "Ball",
                "meta": { "name": "Ball", "enabled": true },
                "props": { "intensity": 0.5 },
                "data": {},
                "$props": {
                    "intensity": { "id": "doc-intensity", "type": "float", "min": 0, "max": 1 },
                },
            },
        }),
    );
    document["inputs"]["focusedInput"] = json!({
        "id": "doc-intensity", "title": "Ball.intensity",
    });
    document["midi"] = json!({
        "devices": {
            "doc-ctrl-Launchpad-Novation": {
                "id": "doc-ctrl-Launchpad-Novation",
                "name": "Launchpad",
                "manufacturer": "Novation",
                "channelData": { "1": { "144": 64 } },
                "listenForInput": true,
                "listenForClock": true,
                "ccLatch": false,
                "noteOnLatch": false,
            },
        },
        "learning": false,
    });
    app.dispatch("scenes/loadSceneFromJson", json!({ "scene": document }));

    let state = app.wait_for_state(Duration::from_secs(30), |state| {
        state["appliedSceneId"] == json!("e2e-midi-scene")
            && state["midi"]["devices"]
                .get("doc-ctrl-Launchpad-Novation")
                .is_some()
    });
    assert_eq!(
        state["midi"]["devices"]["doc-ctrl-Launchpad-Novation"]["channelData"]["1"]["144"],
        json!(64),
        "the Scene's MIDI channel data is live: {state}"
    );
    assert!(
        state["midi"]["devices"]
            .get("live-ctrl-Live-Acme")
            .is_none(),
        "MIDI state is replaced, not merged: {state}"
    );

    let live_input_id = state["modules"]
        .as_object()
        .expect("modules")
        .values()
        .find(|module| module["$moduleName"] == json!("Ball"))
        .and_then(|module| module["$props"]["intensity"]["id"].as_str())
        .expect("the rebuilt intensity input")
        .to_string();
    assert_ne!(live_input_id, "doc-intensity", "a fresh input id is minted");
    assert_eq!(
        state["inputs"]["focusedInput"],
        json!({ "id": live_input_id, "title": "Ball.intensity" }),
        "the saved focus is remapped onto the rebuilt input: {state}"
    );

    app.quit();
    app.assert_exit(0);
}

/// 57174b5b + 457c846a: Group rename and Module duplication were dispatch
/// wildcards in native mode; both must survive an engine snapshot restart.
#[test]
fn group_rename_and_module_duplication_survive_a_snapshot_restart() {
    let root = e2e::temp_root("vmodv-e2e-group-edit");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");

    let mut first = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    first.wait_ready();

    first.dispatch(
        "groups/createGroup",
        json!({
            "id": "e2e-group", "name": "Before", "enabled": 1,
        }),
    );
    let module = first.dispatch(
        "modules/makeActiveModule",
        json!({ "moduleName": "Ball", "moduleMeta": { "enabled": true } }),
    );
    let source_id = module["$id"]
        .as_str()
        .expect("the active Module id")
        .to_string();
    first.dispatch(
        "groups/ADD_MODULE_TO_GROUP",
        json!({
            "groupId": "e2e-group", "moduleId": source_id,
        }),
    );
    let duplicate = first.dispatch(
        "groups/duplicateModule",
        json!({ "groupId": "e2e-group", "moduleId": source_id }),
    );
    let duplicate_id = duplicate["$id"]
        .as_str()
        .expect("duplicateModule returns the new Module")
        .to_string();
    assert_ne!(
        duplicate_id, source_id,
        "the copy has its own id: {duplicate}"
    );
    assert_eq!(duplicate["$moduleName"], json!("Ball"));
    first.dispatch(
        "groups/updateGroupName",
        json!({ "groupId": "e2e-group", "name": "After" }),
    );

    let state = first.wait_for_state(Duration::from_secs(20), |state| {
        state["groups"]
            .as_array()
            .is_some_and(|groups| groups.iter().any(|group| group["name"] == json!("After")))
    });
    let renamed = state["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .find(|group| group["id"] == json!("e2e-group"))
        .expect("the renamed Group");
    assert_eq!(
        renamed["modules"]
            .as_array()
            .expect("group modules")
            .iter()
            .filter(|id| **id == json!(source_id) || **id == json!(duplicate_id))
            .count(),
        2,
        "the source and its duplicate are both in the Group: {renamed}"
    );

    first.save();
    first.quit();
    first.assert_exit(0);

    let mut second = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    second.wait_ready();
    let state = second.state();
    let restored = state["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .find(|group| group["id"] == json!("e2e-group"))
        .expect("the Group survives the restart");
    assert_eq!(restored["name"], json!("After"), "{restored}");
    assert_eq!(
        restored["modules"].as_array().expect("group modules").len(),
        2,
        "both Module instances are restored: {restored}"
    );
    assert_eq!(
        state["modules"][&duplicate_id]["$moduleName"],
        json!("Ball"),
        "the duplicate itself is restored: {state}"
    );

    second.quit();
    second.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}

/// BUGS.md "paste" + 236a3bf9: the engine half of copy/paste, driven directly
/// through the dispatch the menu sends, including its durable persistence.
#[test]
fn copying_a_scene_cell_over_the_socket_updates_the_target_and_persists() {
    let root = e2e::temp_root("vmodv-e2e-cell-copy");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    seed_scenes(
        &user_data,
        vec![
            scene(
                "scene-a",
                "Copy Source",
                vec![group("src-group", "Foreground", &["src-ball"])],
                json!({ "src-ball": module("src-ball", "Ball") }),
            ),
            scene(
                "scene-b",
                "Copy Target",
                vec![group("dst-group", "Foreground", &["dst-bands"])],
                json!({ "dst-bands": module("dst-bands", "Bands") }),
            ),
        ],
        json!({ "enabled": false, "rowNames": [] }),
    );

    let mut app = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    app.wait_ready();
    let state = app.wait_for_state(Duration::from_secs(30), |state| {
        scene_with_id(state, "scene-a").is_some() && scene_with_id(state, "scene-b").is_some()
    });
    assert_eq!(
        cell_module_names(&scene_with_id_owned(&state, "scene-b"), 0),
        ["Bands"],
        "the target starts with its own Module: {state}"
    );

    let copied = app.dispatch(
        "scenes/copySceneCellToScene",
        json!({
            "fromSceneId": "scene-a",
            "toSceneId": "scene-b",
            "fromRowIndex": 0,
            "toRowIndex": 0,
        }),
    );
    assert_eq!(copied["id"], json!("scene-b"), "{copied}");
    assert_eq!(
        cell_module_names(&copied, 0),
        ["Ball"],
        "the whole source cell (Group + Module snapshot) is copied: {copied}"
    );
    assert_eq!(
        copied["modules"]["src-ball"]["$moduleName"],
        json!("Ball"),
        "the Module snapshot travels with its original id: {copied}"
    );

    let empty = app.dispatch(
        "scenes/copySceneCellToScene",
        json!({
            "fromSceneId": "scene-a",
            "toSceneId": "scene-b",
            "fromRowIndex": 9,
            "toRowIndex": 0,
        }),
    );
    assert_eq!(
        empty,
        Value::Null,
        "an empty source cell is a no-op: {empty}"
    );

    app.quit();
    app.assert_exit(0);

    let saved = read_scenes_file(&user_data);
    let target = saved["scenes"]
        .as_array()
        .expect("saved scenes")
        .iter()
        .find(|scene| scene["id"] == json!("scene-b"))
        .expect("the copied Scene is saved");
    assert_eq!(
        cell_module_names(target, 0),
        ["Ball"],
        "the copied cell is durable: {target}"
    );
    let _ = fs::remove_dir_all(root);
}

/// d37e006d: the authoritative live-row stamps must be per row when
/// performance mode is off — applying two rows from different Scenes must not
/// report either Scene as owning the other's row.
#[test]
fn live_row_stamps_stay_per_row_with_performance_disabled() {
    let root = e2e::temp_root("vmodv-e2e-live-rows");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    seed_scenes(
        &user_data,
        vec![
            scene(
                "scene-a",
                "Row A",
                vec![group("a-group", "Background", &["a-ball"])],
                json!({ "a-ball": module("a-ball", "Ball") }),
            ),
            scene(
                "scene-b",
                "Row B",
                vec![Value::Null, group("b-group", "Effects", &["b-bands"])],
                json!({ "b-bands": module("b-bands", "Bands") }),
            ),
        ],
        json!({ "enabled": false, "rowNames": ["Background", "Effects"] }),
    );

    let mut app = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    app.wait_ready();
    no_transition(&mut app);
    assert_eq!(
        app.state()["performance"]["enabled"],
        json!(false),
        "the test really runs with performance mode off"
    );

    for (scene_id, row) in [("scene-a", 0), ("scene-b", 1)] {
        let applied = app.dispatch(
            "scenes/applySceneRow",
            json!({ "sceneId": scene_id, "rowIndex": row }),
        );
        assert_eq!(applied["sceneId"], json!(scene_id), "{applied}");
    }
    let state = app.wait_for_state(Duration::from_secs(30), |state| {
        live_cell_groups(state, "scene-a", 0).len() == 1
            && live_cell_groups(state, "scene-b", 1).len() == 1
    });
    assert!(
        live_cell_groups(&state, "scene-a", 1).is_empty(),
        "Scene A owns only its own row: {state}"
    );
    assert!(
        live_cell_groups(&state, "scene-b", 0).is_empty(),
        "Scene B owns only its own row: {state}"
    );
    assert_eq!(
        live_module_names(&state, live_cell_groups(&state, "scene-a", 0)[0]),
        ["Ball"]
    );
    assert_eq!(
        live_module_names(&state, live_cell_groups(&state, "scene-b", 1)[0]),
        ["Bands"]
    );

    app.quit();
    app.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}

/// d35241f6: the per-cell disable flag lives in `performance.disabledCells`
/// and is saved beside the Scenes, not in the in-memory runtime only.
#[test]
fn a_disabled_scene_cell_is_saved_and_rehydrated() {
    let root = e2e::temp_root("vmodv-e2e-cell-enabled");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    seed_scenes(
        &user_data,
        vec![scene(
            "scene-a",
            "Cell Toggle",
            vec![group("a-group", "Background", &["a-ball"])],
            json!({ "a-ball": module("a-ball", "Ball") }),
        )],
        json!({ "enabled": false, "rowNames": [] }),
    );

    let mut first = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    first.wait_ready();
    let toggled = first.dispatch(
        "scenes/setCellEnabled",
        json!({ "sceneId": "scene-a", "rowIndex": 0, "enabled": false }),
    );
    assert_eq!(
        toggled,
        json!({ "sceneId": "scene-a", "rowIndex": 0, "enabled": false }),
        "{toggled}"
    );
    assert_eq!(
        first.state()["performance"]["disabledCells"],
        json!({ "scene-a": [0] }),
        "the cell flag is in the live performance snapshot"
    );
    let empty_cell = first.dispatch(
        "scenes/setCellEnabled",
        json!({ "sceneId": "scene-a", "rowIndex": 9, "enabled": false }),
    );
    assert_eq!(
        empty_cell["reason"],
        json!("empty-cell"),
        "an empty cell cannot be disabled: {empty_cell}"
    );
    first.quit();
    first.assert_exit(0);

    assert_eq!(
        read_scenes_file(&user_data)["performance"]["disabledCells"],
        json!({ "scene-a": [0] }),
        "the cell flag is written to scenes.json"
    );

    let mut second = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    second.wait_ready();
    assert_eq!(
        second.state()["performance"]["disabledCells"],
        json!({ "scene-a": [0] }),
        "the cell flag is hydrated by a new process"
    );
    second.quit();
    second.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}

/// 6c627812: `dataTypes/createType { type: "tween" }` used to be a UI-only
/// wildcard; the real store must now mint a descriptor and persist it.
#[test]
fn creating_a_tween_data_type_mints_a_live_tween() {
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();
    app.wait_ready();

    let created = app.dispatch(
        "dataTypes/createType",
        json!({ "type": "tween", "args": { "data": [[0], [1]], "duration": 250 } }),
    );
    let id = created["id"]
        .as_str()
        .expect("a tween descriptor is returned")
        .to_string();
    assert_eq!(created["duration"], json!(250), "{created}");
    assert_eq!(created["data"], json!([[0], [1]]), "{created}");
    assert_eq!(
        app.dispatch("dataTypes/createType", json!({ "type": "unknown" })),
        Value::Null,
        "other data types stay UI-only"
    );

    let state = app.wait_for_state(Duration::from_secs(20), |state| {
        state["tweens"]
            .as_array()
            .is_some_and(|tweens| tweens.iter().any(|tween| tween["id"] == json!(id)))
    });
    assert!(
        state["tweens"]
            .as_array()
            .is_some_and(|tweens| !tweens.is_empty()),
        "the created tween is in the authoritative state: {state}"
    );

    app.quit();
    app.assert_exit(0);
}

/// fc8371c4: the legacy checkboxes commit 1/0 rather than booleans; the
/// engine coerces them for both Group flags and Module meta.
#[test]
fn numeric_checkbox_flags_decode_as_booleans_over_the_socket() {
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();
    app.wait_ready();
    app.dispatch(
        "groups/createGroup",
        json!({
            "id": "e2e-flags", "name": "Flags", "enabled": 1,
        }),
    );
    app.dispatch(
        "groups/UPDATE_GROUP",
        json!({
            "groupId": "e2e-flags",
            "data": { "clearing": 1, "pipeline": 1, "inherit": 0, "hidden": 1 },
        }),
    );

    let module = app.dispatch("modules/makeActiveModule", json!({ "moduleName": "Bands" }));
    let module_id = module["$id"]
        .as_str()
        .expect("the active Module id")
        .to_string();
    app.dispatch(
        "modules/UPDATE_ACTIVE_MODULE_META",
        json!({ "id": module_id, "metaKey": "enabled", "data": 0 }),
    );

    let state = app.wait_for_state(Duration::from_secs(20), |state| {
        state["modules"][&module_id]["meta"]["enabled"].is_boolean()
    });
    let group = state["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .find(|group| group["id"] == json!("e2e-flags"))
        .expect("the flagged Group");
    assert_eq!(group["clearing"], json!(true), "{group}");
    assert_eq!(group["pipeline"], json!(true), "{group}");
    assert_eq!(group["hidden"], json!(true), "{group}");
    assert_eq!(group["inherit"], json!(false), "{group}");
    assert_eq!(
        state["modules"][&module_id]["meta"]["enabled"],
        json!(false),
        "a numeric 0 disables the Module: {state}"
    );

    app.dispatch(
        "modules/UPDATE_ACTIVE_MODULE_META",
        json!({ "id": module_id, "metaKey": "enabled", "data": 1 }),
    );
    assert_eq!(
        app.state()["modules"][&module_id]["meta"]["enabled"],
        json!(true),
        "a numeric 1 enables it again"
    );

    app.quit();
    app.assert_exit(0);
}

/// 011e432a: the control socket is private, and malformed requests (a bad
/// screenshot path or an out-of-range size) must be reported without killing
/// the shell or leaving its client unable to reconnect.
#[test]
fn the_control_socket_is_private_and_survives_invalid_requests() {
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();
    app.wait_ready();

    let mode = fs::metadata(app.socket_path())
        .expect("the control socket exists")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "the control socket is owner-only");

    let bad_path = app.call(json!({
        "cmd": "screenshot",
        "path": "/definitely/not/a/real/directory/frame.png",
        "width": 320,
        "height": 180,
    }));
    assert!(bad_path.is_err(), "a bad path is an error: {bad_path:?}");

    let zero_size = app.call(json!({
        "cmd": "screenshot",
        "path": app.temp_dir().join("zero.png").to_string_lossy(),
        "width": 0,
        "height": 180,
    }));
    assert!(
        zero_size.is_err(),
        "a zero width is an error: {zero_size:?}"
    );

    assert_eq!(app.ping()["pong"], json!(true), "the shell still answers");
    app.quit();
    app.assert_exit(0);
}
