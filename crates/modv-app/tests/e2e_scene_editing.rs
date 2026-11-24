//! Real-binary coverage for Scene-grid actions that were missing in the
//! native shell (aa2814a6, 1236c374, bda55128) and for the startup CLI flags
//! wired in eb00dd3b / 2ddc89b4.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_scene_editing -- --test-threads=1"
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
        "minSeconds": 600,
        "maxSeconds": 900,
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

fn scene_in(state: &Value, scene_id: &str) -> Value {
    state["scenes"]
        .as_array()
        .expect("scenes array")
        .iter()
        .find(|scene| scene["id"] == json!(scene_id))
        .unwrap_or_else(|| panic!("Scene {scene_id} missing: {state}"))
        .clone()
}

/// The saved cell for `row`. Null holes keep a Scene's `groups` array aligned
/// with the named rows, but a newly created cell can also be appended, so find
/// it by its `sceneRow` stamp before falling back to the positional slot.
fn saved_cell_group(document: &Value, row: usize) -> Value {
    document["groups"]
        .as_array()
        .and_then(|groups| {
            groups
                .iter()
                .find(|group| group["sceneRow"] == json!(row))
                .cloned()
        })
        .or_else(|| document["groups"].get(row).cloned())
        .unwrap_or(Value::Null)
}

fn live_groups<'a>(state: &'a Value, scene_id: &str) -> Vec<&'a Value> {
    state["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .filter(|group| group["sceneId"] == json!(scene_id))
        .collect()
}

fn read_scenes_file(user_data: &Path) -> Value {
    let path = user_data.join("scenes.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()))
}

/// aa2814a6 / 2213: the Scene list actions used by the grid used to fall
/// through the dispatch wildcard in native mode.
#[test]
fn scene_list_actions_work_over_the_socket_and_persist() {
    let root = e2e::temp_root("vmodv-e2e-scene-list");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    seed_scenes(
        &user_data,
        vec![scene(
            "scene-a",
            "Alpha",
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

    let empty = first.dispatch("scenes/addEmptyScene", json!({ "name": "Empty" }));
    let empty_id = empty["id"]
        .as_str()
        .expect("addEmptyScene returns a new Scene")
        .to_string();
    assert_eq!(empty["name"], json!("Empty"), "{empty}");
    assert_eq!(empty["groups"], json!([]), "{empty}");

    let renamed = first.dispatch(
        "scenes/updateSceneName",
        json!({ "sceneId": empty_id, "name": "  Renamed  " }),
    );
    assert_eq!(renamed["name"], json!("Renamed"), "{renamed}");
    assert_eq!(
        first.dispatch(
            "scenes/updateSceneName",
            json!({ "sceneId": "scene-a", "name": "renamed" }),
        ),
        Value::Null,
        "case-insensitive duplicate names are rejected"
    );

    let disabled = first.dispatch(
        "scenes/setSceneEnabled",
        json!({ "sceneId": empty_id, "enabled": false }),
    );
    assert_eq!(disabled["enabled"], json!(false), "{disabled}");

    let timed = first.dispatch(
        "scenes/updateSceneTiming",
        json!({ "sceneId": empty_id, "minSeconds": 4, "maxSeconds": 9 }),
    );
    assert_eq!(timed["timingOverride"], json!(true), "{timed}");
    assert_eq!(timed["minSeconds"], json!(4));
    assert_eq!(timed["maxSeconds"], json!(9));

    assert_eq!(
        first.dispatch(
            "scenes/moveScene",
            json!({ "sceneId": empty_id, "toIndex": 0 }),
        ),
        json!(true),
        "the empty Scene is moved to the head of the list"
    );
    first.dispatch("scenes/resetSceneTiming", json!({ "sceneId": empty_id }));

    let state = first.wait_for_state(Duration::from_secs(20), |state| {
        state["scenes"].as_array().is_some_and(|scenes| {
            scenes
                .first()
                .is_some_and(|scene| scene["id"] == json!(empty_id))
        })
    });
    assert_eq!(scene_in(&state, &empty_id)["name"], json!("Renamed"));
    assert_eq!(scene_in(&state, &empty_id)["enabled"], json!(false));
    assert_eq!(scene_in(&state, &empty_id)["timingOverride"], json!(false));
    assert_eq!(scene_in(&state, &empty_id)["minSeconds"], json!(8));

    first.quit();
    first.assert_exit(0);

    let saved = read_scenes_file(&user_data);
    let persisted = saved["scenes"]
        .as_array()
        .expect("saved scenes")
        .iter()
        .find(|scene| scene["id"] == json!(empty_id))
        .expect("the edited Scene is saved");
    assert_eq!(persisted["name"], json!("Renamed"), "{persisted}");
    assert_eq!(persisted["enabled"], json!(false));
    assert_eq!(persisted["timingOverride"], json!(false));

    let mut second = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    second.wait_ready();
    let restored = scene_in(&second.state(), &empty_id);
    assert_eq!(restored["name"], json!("Renamed"), "{restored}");
    assert_eq!(restored["enabled"], json!(false));
    assert_eq!(
        restored["timingOverride"],
        json!(false),
        "the reset timing survives the restart: {restored}"
    );

    let removed = second.dispatch("scenes/removeScene", json!({ "sceneId": empty_id }));
    assert_eq!(removed["removed"], json!(true), "{removed}");
    assert!(
        !second.state()["scenes"]
            .as_array()
            .expect("scenes")
            .iter()
            .any(|scene| scene["id"] == json!(empty_id)),
        "the removed Scene leaves the store"
    );
    second.quit();
    second.assert_exit(0);
    let _ = fs::remove_dir_all(root);
}

/// 1236c374 + bda55128: creating, capturing and clearing a cell used to be
/// no-ops in the native shell.
#[test]
fn scene_cell_group_create_capture_and_clear_work_over_the_socket() {
    let root = e2e::temp_root("vmodv-e2e-cell-actions");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    seed_scenes(
        &user_data,
        vec![scene(
            "scene-a",
            "Cell Actions",
            vec![Value::Null, Value::Null],
            json!({}),
        )],
        json!({ "enabled": false, "rowNames": ["Background", "Effects"] }),
    );

    let mut app = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    app.wait_ready();
    no_transition(&mut app);

    let created = app.dispatch(
        "scenes/createSceneCellGroup",
        json!({ "sceneId": "scene-a", "rowIndex": 0 }),
    );
    let group_id = created["groupId"]
        .as_str()
        .expect("createSceneCellGroup returns the live Group id")
        .to_string();
    let state = app.wait_for_state(Duration::from_secs(20), |state| {
        state["groups"]
            .as_array()
            .is_some_and(|groups| groups.iter().any(|group| group["id"] == json!(group_id)))
    });
    let live = state["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .find(|group| group["id"] == json!(group_id))
        .expect("the created Group is live");
    assert_eq!(live["sceneId"], json!("scene-a"), "{live}");
    assert_eq!(live["sceneRow"], json!(0), "{live}");

    let with_module = app.dispatch(
        "scenes/addModuleToCell",
        json!({
            "sceneId": "scene-a", "rowIndex": 0, "moduleName": "Ball",
        }),
    );
    let module_id = saved_cell_group(&with_module, 0)["modules"][0]
        .as_str()
        .unwrap_or_else(|| panic!("the Module dropped into the cell: {with_module}"))
        .to_string();
    assert_eq!(
        with_module["modules"][&module_id]["$moduleName"],
        json!("Ball"),
        "{with_module}"
    );

    let captured = app.dispatch(
        "scenes/captureSceneRowToScene",
        json!({ "sceneId": "scene-a", "rowIndex": 0 }),
    );
    assert_eq!(captured["id"], json!("scene-a"), "{captured}");
    assert_eq!(
        saved_cell_group(&captured, 0)["id"],
        json!(group_id),
        "the live Group is captured into the saved cell: {captured}"
    );
    assert_eq!(
        captured["modules"][&module_id]["$moduleName"],
        json!("Ball"),
        "its Module snapshot is captured too: {captured}"
    );

    let cleared = app.dispatch(
        "scenes/clearSceneCell",
        json!({ "sceneId": "scene-a", "rowIndex": 0 }),
    );
    assert_eq!(cleared["id"], json!("scene-a"), "{cleared}");
    assert_eq!(
        saved_cell_group(&cleared, 0)["modules"],
        json!([]),
        "the saved cell is empty again: {cleared}"
    );
    let state = app.wait_for_state(Duration::from_secs(20), |state| {
        state["groups"]
            .as_array()
            .and_then(|groups| groups.iter().find(|group| group["id"] == json!(group_id)))
            .is_some_and(|group| group["modules"] == json!([]))
    });
    assert!(
        state["modules"].get(&module_id).is_none(),
        "clearing releases the live Module instance: {state}"
    );

    app.quit();
    app.assert_exit(0);

    let saved = read_scenes_file(&user_data);
    let persisted = saved["scenes"]
        .as_array()
        .expect("saved scenes")
        .iter()
        .find(|scene| scene["id"] == json!("scene-a"))
        .expect("the Scene is saved");
    assert_eq!(
        saved_cell_group(persisted, 0)["modules"],
        json!([]),
        "the cleared cell is persisted: {persisted}"
    );
    let _ = fs::remove_dir_all(root);
}

/// eb00dd3b / 2ddc89b4: the shell's `--apply-scene` and `--start-performance`
/// flags must leave the engine in the state the CLI promised.
#[test]
fn apply_scene_and_start_performance_flags_set_the_startup_state() {
    let root = e2e::temp_root("vmodv-e2e-startup-flags");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    seed_scenes(
        &user_data,
        vec![scene(
            "cli-scene",
            "E2E CLI Scene",
            vec![
                group("cli-bg", "Background", &["cli-ball"]),
                group("cli-fx", "Effects", &["cli-bands"]),
            ],
            json!({
                "cli-ball": module("cli-ball", "Ball"),
                "cli-bands": module("cli-bands", "Bands"),
            }),
        )],
        json!({ "enabled": false, "rowNames": ["Background", "Effects"] }),
    );

    let mut app = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .apply_scene("E2E CLI Scene")
        .start_performance()
        .spawn();
    app.wait_ready();
    let state = app.wait_for_state(Duration::from_secs(30), |state| {
        state["appliedSceneId"] == json!("cli-scene")
            && state["performance"]["enabled"] == json!(true)
            && live_groups(state, "cli-scene").len() >= 2
    });
    assert_eq!(
        state["performance"]["currentSceneId"],
        json!("cli-scene"),
        "the startup apply is the current Scene: {state}"
    );
    for row in [0, 1] {
        assert!(
            live_groups(&state, "cli-scene")
                .iter()
                .any(|group| group["sceneRow"] == json!(row)),
            "row {row} of the startup Scene is live: {state}"
        );
    }

    app.quit();
    app.assert_exit(0);
    let _ = fs::remove_dir_all(root);
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
