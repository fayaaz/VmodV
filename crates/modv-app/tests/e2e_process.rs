//! Process-level VmodV scenarios ported from the Electron e2e suite.
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
//! `docs/vmodv-e2e.md`.

#![cfg(unix)] // the control harness uses Unix-domain sockets

mod e2e;

use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use e2e::VmodvHarness;

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
/// VmodV single-process shell stands in: it saves the same snapshot the
/// supervisor restores and proves the killed process can be replaced without a
/// stale state/lock.
#[test]
fn engine_skeleton_survives_sigkill_and_restores_state() {
    let root = e2e::temp_root("vmodv-process-skeleton");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");

    let mut first = VmodvHarness::builder()
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

    let mut second = VmodvHarness::builder()
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
    let mut app = VmodvHarness::builder().width(640).height(360).spawn();
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
fn fresh_vmodv_start_creates_exactly_one_default_group() {
    let root = e2e::temp_root("vmodv-process-default-group");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");

    let mut first = VmodvHarness::builder().user_data(&user_data).spawn();
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
    let mut second = VmodvHarness::builder().user_data(&user_data).spawn();
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
    let root = e2e::temp_root("vmodv-process-window-state");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");

    let mut first = VmodvHarness::builder()
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
    let mut second = VmodvHarness::builder()
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
    let root = e2e::temp_root("vmodv-process-scene-open");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");
    let scene_path = root.join("opened-scene.json");
    fs::write(
        &scene_path,
        serde_json::to_vec(&scene_document()).expect("scene JSON"),
    )
    .expect("write the Scene document");

    let mut app = VmodvHarness::builder()
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

/// The Group document saved in one Scene cell (`groups[row]`), or `Null` when
/// the Scene has no such row.
///
/// Rows are positional because a fresh install never names them
/// (`performance.rowNames: []`) — the same reading the `scene_ops` unit tests use.
fn scene_cell(scene: &Value, row: usize) -> Value {
    scene["groups"]
        .as_array()
        .and_then(|groups| groups.get(row).cloned())
        .unwrap_or(Value::Null)
}

/// The Module ids one Scene cell references.
fn cell_module_ids(scene: &Value, row: usize) -> Vec<String> {
    scene_cell(scene, row)["modules"]
        .as_array()
        .map(|modules| {
            modules
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The `$moduleName` of every Module snapshot one Scene cell holds.
fn cell_module_names(scene: &Value, row: usize) -> Vec<String> {
    cell_module_ids(scene, row)
        .iter()
        .map(|id| {
            scene["modules"][id]["$moduleName"]
                .as_str()
                .unwrap_or("<no snapshot>")
                .to_string()
        })
        .collect()
}

/// A saved Scene document out of a `state` snapshot, a `dispatch` result or a
/// `scenes.json` document.
fn scene_with_id(document: &Value, scene_id: &str) -> Option<Value> {
    document
        .get("scenes")
        .and_then(Value::as_array)?
        .iter()
        .find(|scene| scene["id"] == json!(scene_id))
        .cloned()
}

/// The live Groups the running shell has stamped for one Scene cell: a Group
/// carrying this Scene's id and this row index.
///
/// Transition ghosts never reach the `state` snapshot (`create_preset_from_state`
/// drops them), so everything here is a Group the shell actually draws.
fn live_cell_groups<'a>(state: &'a Value, scene_id: &str, row: usize) -> Vec<&'a Value> {
    state["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .filter(|group| group["sceneId"] == json!(scene_id) && group["sceneRow"] == json!(row))
        .collect()
}

/// The single live Group a settled Scene cell is held by, `None` while the cell
/// is mid-crossfade.
///
/// `scenes/applyScene` keeps the Groups it replaces alive and fades them out,
/// so for a moment the row carries two stamped Groups: the outgoing one and the
/// new one. Only a row held by exactly one Group says anything about what the
/// drop became, and the `scenes/tick` the shell runs each frame drives every
/// fade to completion — so the waits below poll for a settled row instead of
/// sleeping.
fn settled_live_cell_group<'a>(state: &'a Value, scene_id: &str, row: usize) -> Option<&'a Value> {
    let groups = live_cell_groups(state, scene_id, row);
    (groups.len() == 1).then_some(groups[0])
}

/// The `$moduleName` of the Modules one live Group owns.
///
/// A missing live Module instance reads as `<not instantiated>` so a cell whose
/// Group lost its Module fails loudly instead of comparing empty lists.
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

/// The Modules of a settled live Scene cell (empty while a fade is in flight).
fn settled_live_cell_module_names(state: &Value, scene_id: &str, row: usize) -> Vec<String> {
    settled_live_cell_group(state, scene_id, row)
        .map(|group| live_module_names(state, group))
        .unwrap_or_default()
}

/// Where the two drops land: the first seeded Scene, in the store's seed order,
/// that is cheap to re-apply inside a process test (no volumetric Point10
/// Module) and has both a populated row and two empty cells — one for the
/// Module drop, one for the Group drop.
struct CellDropTarget {
    scene_id: String,
    scene_name: String,
    /// The empty cell `scenes/addModuleToCell` fills.
    module_row: usize,
    /// The empty cell `scenes/setSceneCellGroup` stamps its Group into.
    group_row: usize,
}

fn cell_drop_target(scenes: &[Value]) -> Option<CellDropTarget> {
    for scene in scenes {
        let (Some(scene_id), Some(scene_name), Some(groups)) = (
            scene["id"].as_str(),
            scene["name"].as_str(),
            scene["groups"].as_array(),
        ) else {
            continue;
        };
        let point10 = scene["modules"]
            .as_object()
            .into_iter()
            .flat_map(|modules| modules.values())
            .filter_map(|module| module["$moduleName"].as_str())
            .any(|name| name.starts_with("Point10"));
        if point10 {
            continue;
        }
        let is_empty_cell = |group: &Value| {
            group
                .get("modules")
                .and_then(Value::as_array)
                .is_some_and(|modules| modules.is_empty())
        };
        let empty_rows: Vec<usize> = groups
            .iter()
            .enumerate()
            .filter(|(_, group)| is_empty_cell(group))
            .map(|(row, _)| row)
            .collect();
        let populated = groups.iter().any(|group| !is_empty_cell(group));
        if !populated || empty_rows.len() < 2 {
            continue;
        }
        return Some(CellDropTarget {
            scene_id: scene_id.to_string(),
            scene_name: scene_name.to_string(),
            module_row: empty_rows[0],
            group_row: empty_rows[1],
        });
    }
    None
}

/// Polls until the shell has applied `target` and both dropped cells are held by
/// exactly one live Group owning `module` / `group_module`.
///
/// The predicate is readiness, not time: a whole-Scene apply crossfades the
/// Groups it replaces, so a cell is only settled once one Group carries its
/// Scene id and row.
fn wait_for_live_cells(
    harness: &mut VmodvHarness,
    target: &CellDropTarget,
    module: &str,
    group_module: &str,
) -> Value {
    harness.wait_for_state(Duration::from_secs(30), |state| {
        state["appliedSceneId"] == json!(target.scene_id)
            && settled_live_cell_module_names(state, &target.scene_id, target.module_row)
                == [module]
            && settled_live_cell_module_names(state, &target.scene_id, target.group_row)
                == [group_module]
    })
}

/// Process-level coverage for the Scene-grid drop gestures of
/// `b59b6fb2 feat(scenes): drag modules and groups into scene cells`.
///
/// The grid turns a Module tile dropped on a cell into `scenes/addModuleToCell`
/// and a Group dropped on a cell into `scenes/setSceneCellGroup`. Both are
/// already covered by `Store` unit tests in `scene_ops` and by egui kittest drag
/// tests that only inspect the `UiAction` the widget emits. The unproven layer —
/// and the only one this harness is scoped to (AGENTS.md: e2e is for behaviour
/// that genuinely needs the full app) — is a running `modv` shell: the
/// control-socket `dispatch` reaching the live Store, the row Group stamping
/// that turns cell content into a live Group/Module, `scenes/applyScene` making
/// it live, the write into `scenes.json`, and a second process over the same
/// userData re-applying the dropped content after a restart.
///
/// Both halves share one launch pair and one seeded Scene so the process cost is
/// paid once, and land in two different empty cells so they stay independent: a
/// single `applyScene` then proves both became live. The Module cell gains a
/// Group minted and stamped by the engine (`sceneId` + `sceneRow`); the Group
/// cell is replaced verbatim by the user's own Group, which is how the two
/// actions differ. Nothing compares pixels and nothing waits on wall-clock time:
/// every assertion reads the authoritative `state` snapshot or the Scene
/// documents in memory and on disk, and every wait is a `wait_for_state`
/// predicate polled against readiness — including the shell finishing the
/// crossfade an apply started.
#[test]
fn scene_cell_module_and_group_drops_apply_and_survive_a_restart() {
    let root = e2e::temp_root("native-scene-cell-drops");
    let user_data = root.join("userData");
    fs::create_dir_all(&user_data).expect("create userData");

    // The Group the VJ drags, and the Module it owns: both named here so the
    // assertions can point at them, and neither used by the seeded Scene.
    let dropped_group_id = "e2e-dropped-group";
    let dropped_group_name = "Dropped Group";
    const DROPPED_MODULE: &str = "Ball";
    const GROUP_MODULE: &str = "Bands";

    let mut first = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    first.wait_ready();

    let state = first.wait_for_state(Duration::from_secs(30), |state| {
        state["runtime"]["frames"].as_u64().unwrap_or(0) > 0
            && state["scenes"]
                .as_array()
                .is_some_and(|scenes| !scenes.is_empty())
    });
    let target = cell_drop_target(state["scenes"].as_array().expect("scenes array"))
        .expect("a seeded Scene with a populated row and two empty cells");

    let before = scene_with_id(&state, &target.scene_id).expect("the seeded Scene is in the store");
    assert_eq!(
        before["name"],
        json!(target.scene_name),
        "the target Scene is the one the scan selected: {before}"
    );
    for row in [target.module_row, target.group_row] {
        assert_eq!(
            cell_module_names(&before, row),
            Vec::<String>::new(),
            "the target cell ({} row {row}) starts empty: {before}",
            target.scene_name
        );
    }
    for module in [DROPPED_MODULE, GROUP_MODULE] {
        assert!(
            state["modules"]
                .as_object()
                .expect("modules")
                .values()
                .all(|instance| instance["$moduleName"] != json!(module)),
            "no {module} is live in the shell before the drops: {state}"
        );
    }

    // — — drag a Module tile into the empty Background cell — —
    let dropped = first.dispatch(
        "scenes/addModuleToCell",
        json!({
            "sceneId": target.scene_id,
            "rowIndex": target.module_row,
            "moduleName": DROPPED_MODULE,
        }),
    );
    assert!(
        dropped.is_object(),
        "the socket dispatch returned the updated Scene document, not {dropped}"
    );
    assert_eq!(
        dropped["id"],
        json!(target.scene_id),
        "the action returns the Scene it edited: {dropped}"
    );
    let module_ids = cell_module_ids(&dropped, target.module_row);
    assert_eq!(
        module_ids.len(),
        1,
        "the empty cell gains exactly the dropped Module: {dropped}"
    );
    let module_id = module_ids[0].clone();
    assert_eq!(
        dropped["modules"][&module_id]["$moduleName"],
        json!(DROPPED_MODULE),
        "the saved cell holds a {DROPPED_MODULE} snapshot: {dropped}"
    );
    assert_eq!(
        scene_cell(&dropped, target.module_row)["sceneId"],
        json!(target.scene_id),
        "the cell's Group is stamped with the Scene id: {}",
        scene_cell(&dropped, target.module_row)
    );
    assert_eq!(
        scene_cell(&dropped, target.module_row)["sceneRow"],
        json!(target.module_row),
        "and with the row index: {}",
        scene_cell(&dropped, target.module_row)
    );

    let state = first.state();
    assert_eq!(
        settled_live_cell_module_names(&state, &target.scene_id, target.module_row),
        [DROPPED_MODULE],
        "one live Group stamped for {} row {} owns the dropped Module: {state}",
        target.scene_name,
        target.module_row
    );
    assert_eq!(
        state["modules"][&module_id]["$moduleName"],
        json!(DROPPED_MODULE),
        "the dropped Module is an active module of the running shell: {state}"
    );

    // — — drag that Group into the other empty cell — —
    let group = first.dispatch(
        "groups/createGroup",
        json!({
            "id": dropped_group_id,
            "name": dropped_group_name,
            "enabled": 1,
        }),
    );
    assert_eq!(
        group["id"],
        json!(dropped_group_id),
        "the drag source is a live Group of the user's own: {group}"
    );
    let group_module = first.dispatch(
        "modules/makeActiveModule",
        json!({ "moduleName": GROUP_MODULE, "moduleMeta": { "enabled": true } }),
    );
    let group_module_id = group_module["$id"]
        .as_str()
        .expect("the drag source Module id")
        .to_string();
    first.dispatch(
        "groups/ADD_MODULE_TO_GROUP",
        json!({ "groupId": dropped_group_id, "moduleId": group_module_id }),
    );

    let stamped = first.dispatch(
        "scenes/setSceneCellGroup",
        json!({
            "sceneId": target.scene_id,
            "rowIndex": target.group_row,
            "groupId": dropped_group_id,
        }),
    );
    assert!(
        stamped.is_object(),
        "the socket dispatch returned the updated Scene document, not {stamped}"
    );
    assert_eq!(
        stamped["id"],
        json!(target.scene_id),
        "the action returns the Scene it edited: {stamped}"
    );
    let cell = scene_cell(&stamped, target.group_row);
    assert_eq!(
        cell["id"],
        json!(dropped_group_id),
        "the cell is now the dragged Group, not the seeded one: {cell}"
    );
    assert_eq!(
        cell["name"],
        json!(dropped_group_name),
        "and it carries the dragged Group's own name: {cell}"
    );
    assert_eq!(
        cell_module_ids(&stamped, target.group_row),
        vec![group_module_id.clone()],
        "the cell holds exactly the dragged Group's Modules: {stamped}"
    );
    assert_eq!(
        cell_module_names(&stamped, target.group_row),
        [GROUP_MODULE],
        "with its Module snapshot saved into the Scene: {stamped}"
    );
    assert_eq!(
        cell_module_names(&stamped, target.module_row),
        [DROPPED_MODULE],
        "the Module drop in the other cell survives the Group stamp: {stamped}"
    );

    // Neither drop reached the layer stack on its own: the Group cell has no
    // live Group until the Scene is applied.
    let state = first.state();
    assert!(
        live_cell_groups(&state, &target.scene_id, target.group_row).is_empty(),
        "stamping a Group into a cell edits the Scene document, not the layer stack: {state}"
    );
    assert!(
        state["groups"]
            .as_array()
            .expect("groups array")
            .iter()
            .any(|group| group["id"] == json!(dropped_group_id)),
        "the dragged Group is a live Group of its own before it is applied: {state}"
    );

    // — — apply the Scene: both cells become live content — —
    first.dispatch("scenes/applyScene", json!({ "sceneId": target.scene_id }));
    let state = wait_for_live_cells(&mut first, &target, DROPPED_MODULE, GROUP_MODULE);
    let module_cell = settled_live_cell_group(&state, &target.scene_id, target.module_row)
        .expect("the Module cell is held by exactly one live Group once the apply settles");
    assert_eq!(
        live_module_names(&state, module_cell),
        [DROPPED_MODULE],
        "applying the Scene re-instantiates the dropped Module: {module_cell}"
    );
    let group_cell = settled_live_cell_group(&state, &target.scene_id, target.group_row)
        .expect("the Group cell is held by exactly one live Group once the apply settles");
    assert_eq!(
        group_cell["name"],
        json!(dropped_group_name),
        "the live Group in that row is the dragged Group itself: {group_cell}"
    );
    assert_eq!(
        live_module_names(&state, group_cell),
        [GROUP_MODULE],
        "and applying the Scene makes the dragged Group's content live in its row: {group_cell}"
    );

    let saved = first.save();
    assert!(first.state_path().is_file(), "{saved}");
    first.quit();
    first.assert_exit(0);

    // — — both drops are written to scenes.json — —
    let scenes_path = user_data.join("scenes.json");
    let text = fs::read_to_string(&scenes_path).unwrap_or_else(|error| {
        panic!(
            "quit must write {}: {error}\n--- logs ---\n{}",
            scenes_path.display(),
            first.logs()
        )
    });
    let on_disk: Value = serde_json::from_str(&text).expect("scenes.json is JSON");
    let saved_scene =
        scene_with_id(&on_disk, &target.scene_id).expect("the Scene is in scenes.json");
    assert_eq!(
        cell_module_names(&saved_scene, target.module_row),
        [DROPPED_MODULE],
        "the dropped Module is persisted into the Scene cell on disk: {saved_scene}"
    );
    assert_eq!(
        scene_cell(&saved_scene, target.group_row)["id"],
        json!(dropped_group_id),
        "the persisted cell still references the dragged Group: {}",
        scene_cell(&saved_scene, target.group_row)
    );
    assert_eq!(
        cell_module_names(&saved_scene, target.group_row),
        [GROUP_MODULE],
        "with the dragged Group's Module snapshot: {saved_scene}"
    );

    // — — a second process over the same userData restores and re-applies — —
    let mut second = VmodvHarness::builder()
        .width(640)
        .height(360)
        .user_data(&user_data)
        .spawn();
    second.wait_ready();
    let state = second.wait_for_state(Duration::from_secs(30), |state| {
        state["runtime"]["frames"].as_u64().unwrap_or(0) > 0
            && scene_with_id(state, &target.scene_id).is_some()
    });
    let restored =
        scene_with_id(&state, &target.scene_id).expect("the Scene is restored on relaunch");
    assert_eq!(
        cell_module_names(&restored, target.module_row),
        [DROPPED_MODULE],
        "the cell still holds the dropped Module after the restart: {restored}"
    );
    assert_eq!(
        cell_module_names(&restored, target.group_row),
        [GROUP_MODULE],
        "and the dragged Group's Module: {restored}"
    );
    assert_eq!(
        scene_cell(&restored, target.group_row)["id"],
        json!(dropped_group_id),
        "the restored cell is still the dragged Group: {}",
        scene_cell(&restored, target.group_row)
    );

    second.dispatch("scenes/applyScene", json!({ "sceneId": target.scene_id }));
    let state = wait_for_live_cells(&mut second, &target, DROPPED_MODULE, GROUP_MODULE);
    let module_cell = settled_live_cell_group(&state, &target.scene_id, target.module_row)
        .expect("the restarted shell holds the Module cell with exactly one live Group");
    assert_eq!(
        live_module_names(&state, module_cell),
        [DROPPED_MODULE],
        "the restarted shell makes the dropped Module live again: {module_cell}"
    );
    let group_cell = settled_live_cell_group(&state, &target.scene_id, target.group_row)
        .expect("the restarted shell holds the Group cell with exactly one live Group");
    assert_eq!(
        group_cell["name"],
        json!(dropped_group_name),
        "after the restart the live Group in that row is still the dragged one: {group_cell}"
    );
    assert_eq!(
        live_module_names(&state, group_cell),
        [GROUP_MODULE],
        "and its content is live again: {group_cell}"
    );

    second.quit();
    second.assert_exit(0);

    let _ = fs::remove_dir_all(root);
}
