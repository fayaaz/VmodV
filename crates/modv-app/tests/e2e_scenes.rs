//! Direct Store/Compositor ports of the Electron Scene end-to-end specs:
//!
//! * `tests/e2e/spec/scenes/performanceGrid.spec.js`
//! * `tests/e2e/spec/scenes/sceneSnapshotsAndPerformance.spec.js`
//! * `tests/e2e/spec/scenes/gallerylessPerformance.spec.js`
//!
//! Everything is driven through the same store actions the worker used; only
//! the row-fade pixel checks need a GPU (lavapipe in the test image, skipped
//! when no adapter is available), so the binary is run with
//! `--test-threads=1`.
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_scenes -- --test-threads=1"
//! ```

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use modv_app::compositor::{Compositor, FrameInfo, RowFadeInfo};
use modv_app::store_ops::Store;
use modv_engine::engine_state::{GALLERY_GROUP_NAME, Group};
use modv_render::{Engine, EngineConfig, wgpu};
use modv_renderers::two_d::AudioFrame;
use serde_json::{Map, Value, json};

const SIZE: u32 = 64;

fn temp_dir(prefix: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "modv-e2e-{prefix}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn no_transition(store: &mut Store) {
    store
        .dispatch(
            "scenes/updatePerformance",
            json!({
                "config": {
                    "enabled": false,
                    "allowedTransitions": ["None"],
                    "transitionSeconds": 0.1,
                    "order": "in-order",
                }
            }),
        )
        .expect("performance config");
}

fn set_transition_seconds(store: &mut Store, seconds: f64) {
    store
        .dispatch(
            "scenes/updatePerformance",
            json!({ "config": { "transitionSeconds": seconds } }),
        )
        .expect("performance config");
}

fn create_group(store: &mut Store, name: &str, extra: Value) -> String {
    let mut payload = Map::new();
    payload.insert("name".to_string(), json!(name));
    payload.insert("enabled".to_string(), json!(1));
    if let Some(object) = extra.as_object() {
        for (key, value) in object {
            payload.insert(key.clone(), value.clone());
        }
    }
    let group = store
        .dispatch("groups/createGroup", Value::Object(payload))
        .expect("createGroup dispatches");
    group["id"].as_str().expect("created Group id").to_string()
}

fn add_module(store: &mut Store, group_id: &str, module_name: &str) -> String {
    let module = store
        .dispatch(
            "modules/makeActiveModule",
            json!({ "moduleName": module_name, "moduleMeta": { "enabled": true } }),
        )
        .expect("makeActiveModule dispatches");
    let module_id = module["$id"]
        .as_str()
        .expect("created Module id")
        .to_string();
    store
        .dispatch(
            "groups/ADD_MODULE_TO_GROUP",
            json!({ "moduleId": module_id, "groupId": group_id }),
        )
        .expect("ADD_MODULE_TO_GROUP dispatches");
    module_id
}

fn save_scene(store: &mut Store, name: &str) -> Value {
    store
        .dispatch("scenes/saveCurrentAsScene", json!({ "name": name }))
        .expect("saveCurrentAsScene dispatches")
}

fn remove_group(store: &mut Store, group_id: &str) {
    store
        .dispatch("groups/removeGroup", json!({ "groupId": group_id }))
        .expect("removeGroup dispatches");
}

fn apply_scene(store: &mut Store, scene_id: &str) {
    store
        .dispatch("scenes/applyScene", json!({ "sceneId": scene_id }))
        .expect("applyScene dispatches");
}

fn apply_scene_row(store: &mut Store, scene_id: &str, row: usize) {
    store
        .dispatch(
            "scenes/applySceneRow",
            json!({ "sceneId": scene_id, "rowIndex": row }),
        )
        .expect("applySceneRow dispatches");
}

fn load_scene(store: &mut Store, scene: Value, apply: bool) {
    store
        .dispatch(
            "scenes/loadSceneFromJson",
            json!({ "scene": scene, "apply": apply, "persist": false }),
        )
        .expect("loadSceneFromJson dispatches");
}

fn tick(store: &mut Store, seconds: f64) {
    store
        .dispatch("scenes/tick", json!({ "delta": seconds * 1000.0 }))
        .expect("scenes/tick dispatches");
}

/// The live Group for `row`, ignoring the outgoing half of a crossfade.
fn scene_row_group(store: &Store, row: usize) -> Option<&Group> {
    store
        .state()
        .groups
        .iter()
        .find(|group| group.scene_row == Some(row) && group.transition_ghost != Some(true))
}

fn module_names(store: &Store, group: &Group) -> Vec<String> {
    group
        .modules
        .iter()
        .filter_map(|module_id| store.state().modules.get(module_id))
        .map(|module| module.module_name.clone())
        .collect()
}

/// A Scene document with one row per `(group name, module name)` entry.
fn scene_document(id: &str, name: &str, rows: &[(&str, &str)]) -> Value {
    let mut groups = Vec::new();
    let mut modules = Map::new();
    for (index, (group_name, module_name)) in rows.iter().enumerate() {
        let group_id = format!("{id}-group-{index}");
        let module_id = format!("{id}-module-{index}");
        let inherit_from = if index == 0 {
            json!(-1)
        } else {
            json!(format!("{id}-group-{}", index - 1))
        };
        groups.push(json!({
            "id": group_id,
            "name": group_name,
            "hidden": false,
            "modules": [module_id],
            "enabled": 1,
            "alpha": 1,
            "inherit": index > 0,
            "inheritFrom": inherit_from,
            "pipeline": false,
            "clearing": true,
            "compositeOperation": "normal",
        }));
        modules.insert(
            module_id.clone(),
            json!({
                "$id": module_id,
                "$moduleName": module_name,
                "meta": { "name": module_name, "enabled": true },
                "props": {},
                "data": {},
                "$props": {},
            }),
        );
    }
    json!({
        "id": id,
        "name": name,
        "groups": groups,
        "modules": Value::Object(modules),
    })
}

/// The active `row -> scene id` view the UI gets from `SET_ACTIVE_ROWS`,
/// rebuilt from the live Group stamps (ghosts skipped).
fn live_rows(store: &Store) -> BTreeMap<usize, String> {
    store
        .state()
        .groups
        .iter()
        .filter(|group| group.transition_ghost != Some(true))
        .filter_map(|group| {
            let row = group.scene_row?;
            let scene = group.scene_id.clone()?;
            Some((row, scene))
        })
        .collect()
}

fn try_engine() -> Option<Engine> {
    match Engine::new(&EngineConfig {
        force_fallback_adapter: true,
    }) {
        Ok(engine) => Some(engine),
        Err(error) => {
            eprintln!("skipping GPU-backed Scene test: no adapter ({error})");
            None
        }
    }
}

/// Read a Group's own rendered canvas through the compositor's row monitor,
/// so an inheritance assertion samples the inheriting Group rather than main
/// (which also contains the Group it inherited from).
fn read_row_monitor(engine: &Engine, compositor: &mut Compositor, group_id: &str) -> Vec<u8> {
    assert!(
        compositor.refresh_row_monitor(group_id),
        "Group {group_id} rendered a texture"
    );
    let view = compositor
        .row_monitor_view(group_id)
        .expect("row monitor view");
    let texture = view.texture();

    let unpadded_bytes_per_row = SIZE * 4;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = engine.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("modv-e2e-row-monitor-read-back"),
        size: u64::from(padded_bytes_per_row) * u64::from(SIZE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = engine
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("modv-e2e-row-monitor-read-back"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    engine.queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    engine
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("row monitor read-back polls");
    receiver
        .recv()
        .expect("row monitor map callback")
        .expect("row monitor buffer maps");

    let mut pixels = Vec::with_capacity((unpadded_bytes_per_row * SIZE) as usize);
    {
        let mapped = slice.get_mapped_range().expect("row monitor mapped range");
        for row in 0..SIZE {
            let start = (row * padded_bytes_per_row) as usize;
            pixels.extend_from_slice(&mapped[start..start + unpadded_bytes_per_row as usize]);
        }
    }
    buffer.unmap();
    pixels
}

fn frame_info(time_seconds: f32, frame_index: u64, row_fades: Vec<RowFadeInfo>) -> FrameInfo {
    FrameInfo {
        time_seconds,
        frame_index,
        date: [0.0; 4],
        audio: AudioFrame::default(),
        row_fades,
        scene_transition: None,
    }
}

fn centre_pixel(pixels: &[u8]) -> [u8; 4] {
    let index = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

// ————————————————— performanceGrid.spec.js —————————————————

/// `applying a row Scene only replaces that row`: row 0 takes Scene B's
/// Group/Modules, while row 1's live Group id and Module list stay untouched.
#[test]
fn applying_a_row_scene_only_replaces_that_row() {
    let mut store = Store::new();
    no_transition(&mut store);

    let a0 = create_group(&mut store, "Row Apply A0", json!({}));
    add_module(&mut store, &a0, "Ball");
    let a1 = create_group(&mut store, "Row Apply A1", json!({}));
    add_module(&mut store, &a1, "Bands");
    let scene_a = save_scene(&mut store, "Row Apply A");
    let scene_a_id = scene_a["id"].as_str().expect("Scene A id").to_string();

    remove_group(&mut store, &a0);
    remove_group(&mut store, &a1);

    let b0 = create_group(&mut store, "Row Apply B0", json!({}));
    add_module(&mut store, &b0, "Bands");
    let b1 = create_group(&mut store, "Row Apply B1", json!({}));
    add_module(&mut store, &b1, "Ball");
    let scene_b = save_scene(&mut store, "Row Apply B");
    let scene_b_id = scene_b["id"].as_str().expect("Scene B id").to_string();

    remove_group(&mut store, &b0);
    remove_group(&mut store, &b1);

    apply_scene(&mut store, &scene_a_id);

    let before_row0 = scene_row_group(&store, 0).expect("row 0 live").clone();
    let before_row1 = scene_row_group(&store, 1).expect("row 1 live").clone();
    assert_eq!(before_row0.name, "Row Apply A0");
    assert_eq!(module_names(&store, &before_row0), ["Ball"]);
    assert_eq!(before_row1.name, "Row Apply A1");
    assert_eq!(module_names(&store, &before_row1), ["Bands"]);
    assert_eq!(live_rows(&store).get(&0), Some(&scene_a_id));
    assert_eq!(live_rows(&store).get(&1), Some(&scene_a_id));

    let row_one_modules = before_row1.modules.clone();

    apply_scene_row(&mut store, &scene_b_id, 0);

    let after_row0 = scene_row_group(&store, 0)
        .expect("row 0 live after")
        .clone();
    let after_row1 = scene_row_group(&store, 1)
        .expect("row 1 live after")
        .clone();
    // Native row applies mint a fresh Group id (`applySceneRowNow` reuses the
    // old id in JS); the other row must keep both its id and its Modules.
    assert_ne!(after_row0.id, before_row0.id);
    assert_eq!(after_row0.name, "Row Apply B0");
    assert_eq!(module_names(&store, &after_row0), ["Bands"]);
    assert_ne!(after_row0.modules, before_row0.modules);
    assert_eq!(after_row1.id, before_row1.id);
    assert_eq!(after_row1.name, "Row Apply A1");
    assert_eq!(after_row1.modules, row_one_modules);
    assert_eq!(live_rows(&store).get(&0), Some(&scene_b_id));
    assert_eq!(live_rows(&store).get(&1), Some(&scene_a_id));

    assert!(
        store
            .scenes_state()
            .scenes
            .iter()
            .any(|scene| scene["id"] == json!(scene_a_id))
    );
    assert!(
        store
            .scenes_state()
            .scenes
            .iter()
            .any(|scene| scene["id"] == json!(scene_b_id))
    );
}

/// `a column Scene apply clears a row the Scene does not define`: applying
/// Scene C (one row) removes Scene A's row-1 Group and its active-row marker.
#[test]
fn a_column_scene_apply_clears_a_row_the_scene_does_not_define() {
    let mut store = Store::new();
    no_transition(&mut store);

    let a0 = create_group(&mut store, "Column Rows A0", json!({}));
    add_module(&mut store, &a0, "Ball");
    let a1 = create_group(&mut store, "Column Rows A1", json!({}));
    add_module(&mut store, &a1, "Bands");
    let scene_a = save_scene(&mut store, "Column Rows A");
    let scene_a_id = scene_a["id"].as_str().expect("Scene A id").to_string();
    remove_group(&mut store, &a0);
    remove_group(&mut store, &a1);

    let c0 = create_group(&mut store, "Column Rows C0", json!({}));
    add_module(&mut store, &c0, "Ball");
    let scene_c = save_scene(&mut store, "Column Rows C");
    let scene_c_id = scene_c["id"].as_str().expect("Scene C id").to_string();
    remove_group(&mut store, &c0);

    apply_scene(&mut store, &scene_a_id);
    assert_eq!(
        scene_row_group(&store, 0).map(|group| group.name.as_str()),
        Some("Column Rows A0")
    );
    assert_eq!(
        scene_row_group(&store, 1).map(|group| group.name.as_str()),
        Some("Column Rows A1")
    );

    apply_scene(&mut store, &scene_c_id);

    let live: Vec<&Group> = store
        .state()
        .groups
        .iter()
        .filter(|group| group.transition_ghost != Some(true))
        .collect();
    assert_eq!(live.len(), 1, "the undefined row is cleared: {live:?}");
    assert_eq!(live[0].name, "Column Rows C0");
    assert_eq!(live[0].scene_row, Some(0));
    assert_eq!(live[0].scene_id.as_deref(), Some(scene_c_id.as_str()));
    assert!(
        scene_row_group(&store, 1).is_none(),
        "row 1 has no Scene-stamped Group after the column apply"
    );
    let rows = live_rows(&store);
    assert_eq!(rows.get(&0), Some(&scene_c_id));
    assert!(!rows.contains_key(&1));
}

/// `a row fade keeps the outgoing visuals animating`: the ghost Group keeps
/// its Modules active and the compositor keeps updating them, so the outgoing
/// half changes between frames instead of freezing.
#[test]
fn a_row_fade_keeps_the_outgoing_module_animating() {
    let mut store = Store::new();
    no_transition(&mut store);

    let a0 = create_group(&mut store, "Fade Motion A0", json!({}));
    add_module(&mut store, &a0, "Counter");
    let scene_a = save_scene(&mut store, "Fade Motion A");
    let scene_a_id = scene_a["id"].as_str().expect("Scene A id").to_string();
    remove_group(&mut store, &a0);

    let b0 = create_group(&mut store, "Fade Motion B0", json!({}));
    add_module(&mut store, &b0, "Texture 2D");
    let scene_b = save_scene(&mut store, "Fade Motion B");
    let scene_b_id = scene_b["id"].as_str().expect("Scene B id").to_string();
    remove_group(&mut store, &b0);

    apply_scene(&mut store, &scene_a_id);
    set_transition_seconds(&mut store, 2.0);
    apply_scene_row(&mut store, &scene_b_id, 0);

    let ghost = store
        .state()
        .groups
        .iter()
        .find(|group| group.transition_ghost == Some(true) && group.scene_row == Some(0))
        .expect("the outgoing row is a ghost")
        .clone();
    assert_eq!(module_names(&store, &ghost), ["Counter"]);
    let ghost_module_id = ghost.modules[0].clone();

    // Park the fade part-way; the compositor then renders two frames at the
    // same progress, so any pixel change comes from the ghost's module update.
    tick(&mut store, 0.5);
    let transition = store.row_transition(0).expect("row transition").clone();

    let Some(engine) = try_engine() else {
        return;
    };
    let mut compositor = Compositor::new(&engine.device, &engine.queue);
    compositor.resize(SIZE, SIZE);

    let fade = RowFadeInfo {
        row: transition.row,
        group_id: transition.group_id.clone(),
        ghost_group_id: transition.ghost_group_id.clone(),
        from_alpha: transition.from_alpha,
        target_alpha: transition.target_alpha,
        progress: transition.progress(),
    };
    compositor
        .render_frame(store.state(), &frame_info(0.0, 0, vec![fade.clone()]))
        .expect("first mid-fade frame renders");
    let first = compositor
        .read_main_rgba8()
        .expect("first frame reads back");
    compositor
        .render_frame(
            store.state(),
            &frame_info(
                0.1,
                1,
                vec![RowFadeInfo {
                    progress: transition.progress(),
                    ..fade
                }],
            ),
        )
        .expect("second mid-fade frame renders");
    let second = compositor
        .read_main_rgba8()
        .expect("second frame reads back");

    assert_ne!(
        first, second,
        "the outgoing Counter must keep animating during the fade"
    );
    assert!(
        store.state().modules.contains_key(&ghost_module_id),
        "the ghost's Module stays active while the fade runs"
    );

    for _ in 0..20 {
        tick(&mut store, 0.1);
    }
    assert!(store.row_transition(0).is_none(), "fade finished");
    assert!(
        store
            .state()
            .groups
            .iter()
            .all(|group| group.transition_ghost != Some(true)),
        "no ghost survives the fade"
    );
    assert!(
        !store.state().modules.contains_key(&ghost_module_id),
        "the ghost's Module is released with it"
    );
}

/// `a background row fade is visible through the rows that inherit it`: a row
/// that inherits the fading background shows the blended red→blue crossfade
/// rather than the incoming (blue) half immediately.
#[test]
fn a_background_row_fade_is_visible_through_inheriting_rows() {
    let Some(engine) = try_engine() else {
        return;
    };

    let red = json!({ "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 });
    let blue = json!({ "r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0 });

    let mut store = Store::new();
    no_transition(&mut store);
    let mut build = |name: &str, colour: &Value| -> String {
        let background = create_group(
            &mut store,
            &format!("{name} Background"),
            json!({ "inherit": false, "clearing": true }),
        );
        let bar = add_module(&mut store, &background, "Bar");
        store
            .dispatch(
                "modules/updateProp",
                json!({
                    "moduleId": bar,
                    "prop": "thickness",
                    "data": { "value": SIZE as f64 }
                }),
            )
            .expect("thickness");
        for prop in ["color1", "color2"] {
            store
                .dispatch(
                    "modules/updateProp",
                    json!({ "moduleId": bar, "prop": prop, "data": { "value": colour } }),
                )
                .expect("bar colour");
        }
        let foreground = create_group(
            &mut store,
            &format!("{name} Foreground"),
            json!({ "inherit": true, "clearing": true }),
        );
        add_module(&mut store, &foreground, "Texture 2D");
        let scene = save_scene(&mut store, name);
        remove_group(&mut store, &background);
        remove_group(&mut store, &foreground);
        scene["id"].as_str().expect("Scene id").to_string()
    };

    let scene_a_id = build("Inherit Fade A", &red);
    let scene_b_id = build("Inherit Fade B", &blue);

    apply_scene(&mut store, &scene_a_id);
    set_transition_seconds(&mut store, 10.0);
    apply_scene_row(&mut store, &scene_b_id, 0);
    tick(&mut store, 1.0);

    let transition = store.row_transition(0).expect("row transition").clone();
    let fade = RowFadeInfo {
        row: transition.row,
        group_id: transition.group_id.clone(),
        ghost_group_id: transition.ghost_group_id.clone(),
        from_alpha: transition.from_alpha,
        target_alpha: transition.target_alpha,
        progress: transition.progress(),
    };
    assert!(fade.progress > 0.0 && fade.progress < 1.0);

    let mut compositor = Compositor::new(&engine.device, &engine.queue);
    compositor.resize(SIZE, SIZE);
    compositor
        .render_frame(store.state(), &frame_info(0.0, 0, vec![fade]))
        .expect("mid-fade frame renders");
    let follower_id = scene_row_group(&store, 1)
        .expect("the inheriting row is live")
        .id
        .clone();
    let during = centre_pixel(&read_row_monitor(&engine, &mut compositor, &follower_id));
    assert!(
        during[0] > during[2],
        "the inheriting row still shows the outgoing red mid-fade: {during:?}"
    );

    for _ in 0..110 {
        tick(&mut store, 0.1);
    }
    assert!(store.row_transition(0).is_none(), "fade finished");
    compositor
        .render_frame(store.state(), &frame_info(11.0, 2, Vec::new()))
        .expect("post-fade frame renders");
    let follower_id = scene_row_group(&store, 1)
        .expect("the inheriting row survives the fade")
        .id
        .clone();
    let after = centre_pixel(&read_row_monitor(&engine, &mut compositor, &follower_id));
    assert!(
        after[2] > after[0],
        "the completed fade shows the incoming blue: {after:?}"
    );
}

// ————————————————— sceneSnapshotsAndPerformance.spec.js —————————————————

/// `a saved Scene serialises the whole store like a preset`: the saved
/// document carries groups/modules/inputs/expressions/tweens/midi and the
/// focused input, and applying it restores the link + expression on the fresh
/// input id.
#[test]
fn a_saved_scene_serialises_the_whole_store_like_a_preset() {
    let mut store = Store::new();
    no_transition(&mut store);

    let group_id = create_group(&mut store, "Whole Store", json!({}));
    let module_id = add_module(&mut store, &group_id, "Ball");
    let module = store.state().modules.get(&module_id).expect("Ball");
    let input_id = module.dollar_props["intensity"]["id"]
        .as_str()
        .expect("intensity input id")
        .to_string();

    store
        .dispatch(
            "inputs/createInputLink",
            json!({
                "inputId": input_id,
                "type": "getter",
                "location": "meyda/getFeature",
                "source": "meyda",
                "args": ["bass"],
                "min": 0,
                "max": 1,
            }),
        )
        .expect("createInputLink");
    let expression_id = store
        .dispatch(
            "expressions/create",
            json!({ "inputId": input_id, "expression": "inputValue * 0.5" }),
        )
        .expect("expressions/create");
    let expression_id = expression_id.as_str().expect("expression id").to_string();
    store
        .dispatch(
            "midi/ADD_DEVICE",
            json!({ "id": "e2e-whole-store", "name": "E2E Controller", "manufacturer": "modV" }),
        )
        .expect("midi/ADD_DEVICE");
    store
        .dispatch(
            "inputs/setFocusedInput",
            json!({ "id": input_id, "title": "Intensity" }),
        )
        .expect("setFocusedInput");

    let scene = save_scene(&mut store, "Whole Store Scene");
    let scene_id = scene["id"].as_str().expect("Scene id").to_string();

    for key in [
        "groups",
        "modules",
        "inputs",
        "expressions",
        "tweens",
        "midi",
    ] {
        assert!(
            scene.get(key).is_some(),
            "a Scene serialises `{key}` like a preset: {scene}"
        );
    }
    assert_eq!(
        scene["inputs"]["inputLinks"][&input_id],
        json!({
            "id": input_id,
            "type": "getter",
            "location": "meyda/getFeature",
            "source": "meyda",
            "args": ["bass"],
            "min": 0,
            "max": 1,
        })
    );
    assert_eq!(
        scene["expressions"]["assignments"][&expression_id]["expression"],
        json!("inputValue * 0.5")
    );
    assert_eq!(
        scene["expressions"]["assignments"][&expression_id]["inputId"],
        json!(input_id)
    );
    assert!(scene["tweens"].is_array(), "tweens keep the array shape");
    assert!(
        scene["midi"]["devices"]["e2e-whole-store-E2E Controller-modV"].is_object(),
        "MIDI devices are part of the snapshot: {scene}"
    );
    assert_eq!(
        scene["inputs"]["focusedInput"],
        json!({ "id": input_id, "title": "Intensity" })
    );

    // Rebuild the Group so the apply has to restore it.
    remove_group(&mut store, &group_id);
    store
        .dispatch("expressions/remove", json!({ "id": expression_id }))
        .expect("expressions/remove");
    store
        .dispatch("inputs/clearFocusedInput", json!({}))
        .expect("clearFocusedInput");

    apply_scene(&mut store, &scene_id);

    let restored_group = store
        .state()
        .groups
        .iter()
        .find(|group| group.name == "Whole Store")
        .expect("the Group is restored");
    let restored_module_id = restored_group.modules[0].clone();
    let restored_module = store
        .state()
        .modules
        .get(&restored_module_id)
        .expect("the Module is restored");
    let restored_input_id = restored_module.dollar_props["intensity"]["id"]
        .as_str()
        .expect("fresh intensity input id")
        .to_string();
    assert_ne!(restored_input_id, input_id, "a fresh input id is minted");
    let link = store
        .state()
        .input_links
        .get(&restored_input_id)
        .expect("the input link is restored on the fresh id");
    assert_eq!(link.source.as_deref(), Some("meyda"));
    assert_eq!(link.args, Some(vec![json!("bass")]));
    let assignment = store
        .state()
        .expressions
        .values()
        .find(|assignment| assignment.input_id == restored_input_id)
        .expect("the expression is remapped onto the fresh input id");
    assert_eq!(assignment.expression, "inputValue * 0.5");
}

/// `a legacy Scene with an object tweens map still applies`: an older
/// document stores `tweens` as `{ id: tween }`; the tween and its state link
/// are rebuilt on the live input id.
#[test]
fn a_legacy_scene_with_an_object_tweens_map_still_applies() {
    let mut store = Store::new();
    no_transition(&mut store);

    let legacy = json!({
        "id": "legacy-scene",
        "name": "Legacy Scene",
        "enabled": true,
        "groups": [{
            "id": "legacy-group",
            "name": "Legacy Group",
            "hidden": false,
            "modules": ["legacy-ball"],
            "enabled": 1,
            "alpha": 1,
            "inherit": true,
            "inheritFrom": -1,
            "pipeline": false,
            "clearing": true,
            "compositeOperation": "normal",
        }],
        "modules": {
            "legacy-ball": {
                "$id": "legacy-ball",
                "$moduleName": "Ball",
                "meta": { "name": "Ball", "enabled": true },
                "props": { "speed": 2.0 },
                "data": {},
                "$props": {
                    "speed": { "id": "doc-speed", "type": "float" }
                },
            },
        },
        "inputLinks": {
            "doc-speed": {
                "id": "doc-speed",
                "type": "state",
                "location": "tweens.tweens['doc-speed'].value[0]",
                "source": "tween",
                "min": 0,
                "max": 1,
            }
        },
        "tweens": {
            "doc-speed": {
                "id": "doc-speed",
                "data": [[0], [1]],
                "duration": 1000,
                "direction": "alternate",
                "easing": "linear",
                "loop": true,
                "useBpm": true,
                "bpmDivision": 4,
            }
        },
    });
    store
        .dispatch(
            "scenes/loadSceneFromJson",
            json!({ "scene": legacy, "persist": false }),
        )
        .expect("loadSceneFromJson dispatches");

    let group = store
        .state()
        .groups
        .iter()
        .find(|group| group.name == "Legacy Group")
        .expect("legacy Group applies");
    let module_id = group.modules[0].clone();
    let module = store.state().modules.get(&module_id).expect("Ball");
    let live_input_id = module.dollar_props["speed"]["id"]
        .as_str()
        .expect("live speed input id")
        .to_string();
    assert_ne!(live_input_id, "doc-speed", "a fresh input id is minted");

    let tweens = store.state().tweens.as_array().expect("tween array");
    let tween = tweens
        .iter()
        .find(|tween| tween["id"] == json!(live_input_id))
        .expect("the object-map tween is materialised on the live id");
    assert_eq!(tween["useBpm"], json!(true));
    assert_eq!(tween["bpmDivision"], json!(4));
    let link = store
        .state()
        .input_links
        .get(&live_input_id)
        .expect("the tween link is restored");
    assert_eq!(link.source.as_deref(), Some("tween"));
    assert_eq!(
        link.location.as_deref(),
        Some(format!("tweens.tweens['{live_input_id}'].value[0]").as_str())
    );
}

/// `a Scene JSON can be loaded directly`: `loadSceneFromJson` returns the
/// document and (with `apply`) makes it the current Scene with live Groups and
/// Modules.
#[test]
fn a_scene_json_can_be_loaded_directly() {
    let mut store = Store::new();
    no_transition(&mut store);

    let group_id = create_group(&mut store, "Scene JSON", json!({}));
    add_module(&mut store, &group_id, "Ball");
    let scene = save_scene(&mut store, "Scene JSON");
    let scene_id = scene["id"].as_str().expect("Scene id").to_string();
    remove_group(&mut store, &group_id);

    let imported = store
        .dispatch(
            "scenes/loadSceneFromJson",
            json!({ "scene": scene, "persist": false }),
        )
        .expect("loadSceneFromJson dispatches");

    assert_eq!(imported["id"], json!(scene_id));
    assert_eq!(
        store.scenes_state().performance["currentSceneId"],
        json!(scene_id)
    );
    let live = store
        .state()
        .groups
        .iter()
        .find(|group| group.name == "Scene JSON")
        .expect("the Scene's Group is live");
    assert_eq!(live.scene_id.as_deref(), Some(scene_id.as_str()));
    assert_eq!(module_names(&store, live), ["Ball"]);
}

/// `Scenes saved from preset-style Groups apply without swap flags`: the
/// `writeToSwap` action argument never lands on the live Group or the saved
/// Scene, and the rebuilt Scene still applies with its Module.
#[test]
fn scenes_saved_from_preset_style_groups_apply_without_swap_flags() {
    let mut store = Store::new();
    no_transition(&mut store);

    let group_id = create_group(
        &mut store,
        "Swap Flags",
        json!({
            "inherit": false,
            "clearing": true,
            "alpha": 1,
            "compositeOperation": "normal",
            "pipeline": false,
            "writeToSwap": true,
        }),
    );
    store.dispatch("groups/SWAP", json!({})).expect("SWAP");
    let live_group = store
        .state()
        .find_group(&group_id)
        .expect("live Group")
        .clone();
    assert!(
        !live_group.extra.contains_key("writeToSwap"),
        "the live Group does not keep the action flag: {:?}",
        live_group.extra
    );

    let module_id = add_module(&mut store, &group_id, "Ball");
    let scene = save_scene(&mut store, "Swap Flags Scene");
    let scene_id = scene["id"].as_str().expect("Scene id").to_string();
    let saved_group = scene["groups"]
        .as_array()
        .expect("Scene groups")
        .iter()
        .find(|group| group["name"] == json!("Swap Flags"))
        .expect("saved Group");
    assert!(saved_group.get("writeToSwap").is_none());
    assert_eq!(
        saved_group["modules"].as_array().map(Vec::len),
        Some(1),
        "the saved Module is part of the Group"
    );
    assert_eq!(scene["modules"][&module_id]["$moduleName"], json!("Ball"));

    remove_group(&mut store, &group_id);
    apply_scene(&mut store, &scene_id);

    let restored: Vec<&Group> = store
        .state()
        .groups
        .iter()
        .filter(|group| group.name == "Swap Flags")
        .collect();
    assert_eq!(restored.len(), 1);
    let names: Vec<String> = restored
        .iter()
        .flat_map(|group| module_names(&store, group))
        .collect();
    assert_eq!(names, ["Ball"]);
}

/// `legacy autopilot settings hydrate into performance mode`: an old
/// `scenes.json` with an `autopilot` block hydrates the performance config,
/// and the next persist writes `performance` with no `autopilot` key.
#[test]
fn legacy_autopilot_settings_hydrate_into_performance() {
    let dir = temp_dir("autopilot");
    let scenes_path = dir.join("scenes.json");
    std::fs::write(
        &scenes_path,
        serde_json::to_vec(&json!({
            "version": 1,
            "defaultsSeeded": true,
            "scenes": [],
            "autopilot": {
                "enabled": true,
                "minSeconds": 11,
                "maxSeconds": 33,
                "transitionSeconds": 4,
                "allowedTransitions": ["Fade"],
                "order": "random",
            },
        }))
        .expect("scenes.json encodes"),
    )
    .expect("scenes.json write");

    let mut store = Store::with_state_path(Some(&dir.join("engine-state.json")));
    store
        .dispatch("scenes/hydrateScenes", json!({}))
        .expect("hydrateScenes dispatches");

    let performance = store.scenes_state().performance.clone();
    assert_eq!(performance["minSeconds"], json!(11));
    assert_eq!(performance["maxSeconds"], json!(33));
    assert_eq!(performance["transitionSeconds"], json!(4));
    assert_eq!(performance["allowedTransitions"], json!(["Fade"]));
    assert_eq!(performance["order"], json!("random"));
    // Runtime state never survives a restart: the native hydrate resets the
    // mode and the current Scene even when the legacy autopilot had enabled.
    assert_eq!(performance["enabled"], json!(false));
    assert_eq!(performance["currentSceneId"], json!(null));
    assert!(performance.get("autopilot").is_none());

    store
        .dispatch(
            "scenes/updatePerformance",
            json!({
                "enabled": false,
                "minSeconds": 8,
                "maxSeconds": 24,
                "transitionSeconds": 2,
                "allowedTransitions": [],
                "order": "in-order",
            }),
        )
        .expect("updatePerformance dispatches");
    store
        .dispatch("scenes/persistScenes", json!({}))
        .expect("persistScenes dispatches");

    let written: Value =
        serde_json::from_slice(&std::fs::read(&scenes_path).expect("scenes.json written"))
            .expect("scenes.json is JSON");
    assert!(written["performance"].is_object(), "{written}");
    assert!(written.get("autopilot").is_none(), "{written}");
    assert_eq!(written["performance"]["minSeconds"], json!(8));
    assert_eq!(written["performance"]["maxSeconds"], json!(24));

    let _ = std::fs::remove_dir_all(&dir);
}

/// `a per-scene timing override still switches early`: Scene A's 1–2 s
/// override advances before the 5–6 s global window, and resetting the
/// override restores the global bounds.
#[test]
fn a_per_scene_timing_override_still_switches_early() {
    let mut store = Store::new();
    no_transition(&mut store);
    for (id, name) in [("timing-a", "Timing A"), ("timing-b", "Timing B")] {
        load_scene(
            &mut store,
            scene_document(id, name, &[("Background", "Ball")]),
            false,
        );
    }

    store
        .dispatch(
            "scenes/updatePerformance",
            json!({
                "enabled": false,
                "minSeconds": 5,
                "maxSeconds": 6,
                "transitionSeconds": 1,
                "allowedTransitions": ["None"],
                "order": "in-order",
            }),
        )
        .expect("performance config");
    store
        .dispatch(
            "scenes/updateSceneTiming",
            json!({ "sceneId": "timing-a", "minSeconds": 1, "maxSeconds": 2 }),
        )
        .expect("updateSceneTiming dispatches");
    let overridden = store
        .scenes_state()
        .scenes
        .iter()
        .find(|scene| scene["id"] == json!("timing-a"))
        .expect("Scene A");
    assert_eq!(overridden["timingOverride"], json!(true));
    assert_eq!(overridden["minSeconds"], json!(1));
    assert_eq!(overridden["maxSeconds"], json!(2));

    apply_scene(&mut store, "timing-a");
    store
        .dispatch("scenes/startPerformance", json!({}))
        .expect("startPerformance dispatches");

    // Global timing would keep Scene A for at least 5 s; the override's 2 s
    // maximum must switch to Scene B within this window.
    for _ in 0..30 {
        tick(&mut store, 0.1);
    }
    assert_eq!(
        store.scenes_state().performance["currentSceneId"],
        json!("timing-b"),
        "the per-Scene override switched the row early"
    );

    store
        .dispatch("scenes/resetSceneTiming", json!({ "sceneId": "timing-a" }))
        .expect("resetSceneTiming dispatches");
    let reset = store
        .scenes_state()
        .scenes
        .iter()
        .find(|scene| scene["id"] == json!("timing-a"))
        .expect("Scene A");
    assert_eq!(reset["timingOverride"], json!(false));
    assert_eq!(reset["minSeconds"], json!(5));
    assert_eq!(reset["maxSeconds"], json!(6));

    store
        .dispatch("scenes/stopPerformance", json!({}))
        .expect("stopPerformance dispatches");
}

// ————————————————— gallerylessPerformance.spec.js —————————————————

/// `performance rows keep applying without a Gallery Group`: named rows still
/// map, apply and advance when the internal Gallery Group is absent (the
/// legacy `groups/orderByIds` bug appended `undefined` in that layout).
#[test]
fn performance_rows_keep_applying_without_a_gallery_group() {
    let mut store = Store::new();
    // A layout that never mounted the Gallery panel has no Gallery Group.
    assert!(
        store
            .state()
            .groups
            .iter()
            .all(|group| group.name != GALLERY_GROUP_NAME)
    );
    no_transition(&mut store);

    let rows = [
        ("Background", "Bar"),
        ("Foreground", "Texture 2D"),
        ("Effects", "Pixelate"),
    ];
    load_scene(
        &mut store,
        scene_document("no-gallery-a", "No Gallery A", &rows),
        false,
    );
    load_scene(
        &mut store,
        scene_document(
            "no-gallery-b",
            "No Gallery B",
            &[
                ("Background", "Bar"),
                ("Foreground", "Texture 2D"),
                ("Effects", "Smear"),
            ],
        ),
        false,
    );

    store
        .dispatch(
            "scenes/setRowNames",
            json!({ "rowNames": ["webcam", "Background", "Foreground", "Effects"] }),
        )
        .expect("setRowNames dispatches");
    store
        .dispatch(
            "scenes/updatePerformance",
            json!({
                "enabled": false,
                "allowedTransitions": ["None"],
                "transitionSeconds": 0.1,
                "minSeconds": 0.1,
                "maxSeconds": 0.2,
                "order": "in-order",
            }),
        )
        .expect("performance config");

    apply_scene(&mut store, "no-gallery-a");

    assert!(
        store
            .state()
            .groups
            .iter()
            .all(|group| group.name != GALLERY_GROUP_NAME),
        "the apply must not invent a Gallery Group"
    );
    assert!(
        store
            .state()
            .groups
            .iter()
            .all(|group| !group.id.is_empty() && !group.name.is_empty()),
        "no placeholder Group corrupts the layer stack: {:?}",
        store.state().groups
    );
    assert_eq!(
        scene_row_group(&store, 1).map(|group| group.name.as_str()),
        Some("Background")
    );
    assert_eq!(
        scene_row_group(&store, 2).map(|group| group.name.as_str()),
        Some("Foreground")
    );
    let effects = scene_row_group(&store, 3).expect("Effects row applied");
    assert_eq!(effects.name, "Effects");
    assert_eq!(module_names(&store, effects), ["Pixelate"]);

    store
        .dispatch("scenes/startPerformance", json!({}))
        .expect("startPerformance dispatches");
    assert!(
        store.performance_runtime().rows.len() >= 4,
        "the four row names are part of the runtime grid"
    );

    let mut advanced = false;
    for _ in 0..400 {
        tick(&mut store, 0.1);
        if scene_row_group(&store, 3)
            .is_some_and(|group| group.scene_id.as_deref() == Some("no-gallery-b"))
        {
            advanced = true;
            break;
        }
    }
    assert!(
        advanced,
        "the Effects row keeps advancing without a Gallery Group"
    );
    let effects = scene_row_group(&store, 3).expect("Effects row after the advance");
    assert_eq!(effects.name, "Effects");
    assert_eq!(effects.scene_row, Some(3));
    assert_eq!(module_names(&store, effects), ["Smear"]);
    assert_eq!(
        scene_row_group(&store, 1).map(|group| group.name.as_str()),
        Some("Background"),
        "the named rows stay aligned"
    );
    assert!(
        store
            .state()
            .groups
            .iter()
            .all(|group| !group.id.is_empty() && !group.name.is_empty()),
        "the layer stack stays valid after the row advance"
    );

    store
        .dispatch("scenes/stopPerformance", json!({}))
        .expect("stopPerformance dispatches");
    assert!(
        store
            .state()
            .groups
            .iter()
            .all(|group| group.transition_ghost != Some(true)),
        "stopping performance leaves no ghosts"
    );
}
