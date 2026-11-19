//! Native content/dev-runtime port of `tests/e2e/spec/scenes/defaultScenes.spec.js`.
//!
//! The Electron spec audits two things:
//!
//! * the `DEFAULT_SCENES` data itself (no Mesmer in an Effects row, a spread of
//!   registered ISF filters across the Effects rows, the version-3 batch intact
//!   after seeding, the automatic bass/BPM links), and
//! * `scenes.json` hydration for a legacy install whose file only carries
//!   `defaultsSeeded: true`.
//!
//! The data lives in `crates/modv-engine/data/default-scenes.json` and is
//! seeded by [`modv_app::store_ops::Store`], so these tests drive the same
//! store actions the engine does instead of an Electron worker.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_defaults"
//! ```

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use modv_app::module_registry::ModuleRegistry;
use modv_app::scene_ops::SCENES_STORAGE_FILENAME;
use modv_app::store_ops::Store;
use modv_engine::default_scenes::{
    DEFAULTS_VERSION, SceneDocument, default_scenes, get_default_scene_names_up_to, scene_key,
};

/// The filters the e2e spec requires across the Effects rows
/// (defaultScenes.spec.js:11-18).
const REQUIRED_ISF_FILTERS: &[&str] = &[
    "Bloom",
    "VVMotionBlur 3.0",
    "Vignette",
    "Hue-Saturation",
    "Mirror",
    "Slit Scan",
];

fn corpus_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/isf");
    dir.is_dir().then_some(dir)
}

fn temp_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "modv-defaults-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn effects_module_names(scene: &SceneDocument) -> Vec<&str> {
    scene.modules_in_group("Effects")
}

fn seeded_by_name<'a>(scenes: &'a [Value], name: &str) -> Option<&'a Value> {
    scenes
        .iter()
        .find(|scene| scene.get("name").and_then(Value::as_str) == Some(name))
}

/// Seed the full default set through the store, with the ISF corpus registered
/// so a Scene that uses an ISF filter keeps its Modules intact.
fn seeded_store() -> (Store, PathBuf) {
    let dir = temp_dir();
    let state_path = dir.join("engine-state.json");
    let Some(corpus) = corpus_dir() else {
        panic!("the ISF corpus is missing at {}", dir.display());
    };
    let mut store = Store::with_state_path_and_isf(Some(&state_path), Some(&corpus));
    store
        .dispatch("scenes/ensureDefaultScenes", json!({}))
        .expect("default seeding");
    (store, dir)
}

/// `default-scenes.test.js` "does not put the Mesmer 2D Scene in an Effects
/// group" (defaultScenes.spec.js:145-151).
#[test]
fn does_not_put_the_mesmer_scene_in_an_effects_group() {
    let offenders: Vec<&str> = default_scenes()
        .iter()
        .filter(|scene| effects_module_names(scene).contains(&"Mesmer"))
        .map(|scene| scene.name.as_str())
        .collect();

    assert!(
        offenders.is_empty(),
        "Mesmer is a 2D Scene and must not sit in an Effects row: {offenders:?}"
    );
}

/// defaultScenes.spec.js:163-192: the required filters are registered ISF
/// modules, at least 12 Scenes put an ISF filter in Effects, and the required
/// spread is actually used.
#[test]
fn default_scenes_use_a_spread_of_registered_isf_filters_in_effects_groups() {
    let Some(corpus) = corpus_dir() else {
        eprintln!("skipping ISF filter-spread audit: the corpus is missing");
        return;
    };
    let registry = ModuleRegistry::build(Some(&corpus));
    let isf_names: HashSet<&str> = registry
        .isf_definitions()
        .iter()
        .map(|definition| definition.name.as_str())
        .collect();

    for filter in REQUIRED_ISF_FILTERS {
        assert!(
            isf_names.contains(filter),
            "{filter} must be a registered ISF module"
        );
    }

    let scenes_with_filters: Vec<&SceneDocument> = default_scenes()
        .iter()
        .filter(|scene| {
            effects_module_names(scene)
                .iter()
                .any(|name| isf_names.contains(name))
        })
        .collect();
    let mut used_filters: HashSet<&str> = HashSet::new();

    for scene in &scenes_with_filters {
        used_filters.extend(
            effects_module_names(scene)
                .into_iter()
                .filter(|name| isf_names.contains(name)),
        );
    }

    assert!(
        scenes_with_filters.len() >= 12,
        "expected at least 12 default Scenes with an ISF filter in Effects, got {}",
        scenes_with_filters.len()
    );
    assert!(
        used_filters.len() >= REQUIRED_ISF_FILTERS.len(),
        "expected at least {} different ISF filters in Effects, got {}",
        REQUIRED_ISF_FILTERS.len(),
        used_filters.len()
    );

    for filter in REQUIRED_ISF_FILTERS {
        assert!(
            used_filters.contains(filter),
            "{filter} is registered but no Effects row uses it"
        );
    }
}

/// defaultScenes.spec.js:222-259: every version-3 Scene is seeded with the
/// exact Group/Module structure of its definition, in draw order.
#[test]
fn seeds_the_version_3_scenes_with_every_module_intact() {
    let (store, dir) = seeded_store();
    let scenes = store.scenes_state().scenes.clone();
    let batch: Vec<&SceneDocument> = default_scenes()
        .iter()
        .filter(|scene| scene.introduced_version == 3)
        .collect();

    assert!(!batch.is_empty(), "the defaults data has a version-3 batch");

    for definition in batch {
        let scene = seeded_by_name(&scenes, &definition.name)
            .unwrap_or_else(|| panic!("{} was not seeded", definition.name));
        let group_names: Vec<&str> = scene["groups"]
            .as_array()
            .expect("groups array")
            .iter()
            .filter_map(|group| group["name"].as_str())
            .collect();

        for required in ["Foreground", "Effects"] {
            assert!(
                group_names.contains(&required),
                "{} lost its {required} row: {group_names:?}",
                definition.name
            );
        }

        for group in scene["groups"].as_array().expect("groups array") {
            let group_name = group["name"].as_str().expect("group name");
            let definition_group = definition.group(group_name).unwrap_or_else(|| {
                panic!("{}: {group_name} is not in the definition", definition.name)
            });

            let expected: Vec<&str> = definition_group
                .modules
                .iter()
                .map(|module| module.name.as_str())
                .collect();
            let actual: Vec<&str> = group["modules"]
                .as_array()
                .expect("group modules")
                .iter()
                .map(|module_id| {
                    let module_id = module_id.as_str().expect("module id");
                    scene["modules"][module_id]["$moduleName"]
                        .as_str()
                        .unwrap_or_else(|| {
                            panic!(
                                "{}: module {module_id} is missing a $moduleName",
                                definition.name
                            )
                        })
                })
                .collect();

            assert_eq!(
                actual, expected,
                "{}: {group_name} modules changed during seeding",
                definition.name
            );
        }
    }

    let _ = fs::remove_dir_all(&dir);
}

/// defaultScenes.spec.js:550-585: every non-empty default Scene links a
/// control to `bass` and another to a BPM tween. An intentionally blank Scene
/// (Soft Bands) has no Module to pin and is exempt.
#[test]
fn default_scenes_chain_controls_to_bass_and_a_bpm_tween() {
    let (store, dir) = seeded_store();
    let scenes = store.scenes_state().scenes.clone();
    let default_names: HashSet<String> = default_scenes()
        .iter()
        .map(|scene| scene_key(&scene.name))
        .collect();
    let seeded: Vec<&Value> = scenes
        .iter()
        .filter(|scene| {
            scene
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| default_names.contains(&scene_key(name)))
        })
        .collect();

    assert!(
        seeded.len() >= 18,
        "expected at least 18 seeded defaults, got {}",
        seeded.len()
    );

    for scene in seeded {
        let name = scene["name"].as_str().unwrap_or("<unnamed>");
        let modules = scene["modules"].as_object().expect("modules map");

        if modules.is_empty() {
            // The deliberately blank Scene (Soft Bands) has no control to pin.
            continue;
        }

        let links = scene["inputs"]["inputLinks"]
            .as_object()
            .expect("inputLinks map");
        let tweens = scene["tweens"].as_array().expect("tweens array");
        let bass_link = links.values().any(|link| {
            link["source"].as_str() == Some("meyda")
                && link["args"]
                    .as_array()
                    .is_some_and(|args| args.first().and_then(Value::as_str) == Some("bass"))
        });
        let bpm_tween_link = links.values().any(|link| {
            link["source"].as_str() == Some("tween")
                && tweens.iter().any(|tween| {
                    tween["id"] == link["id"] && tween["useBpm"].as_bool() == Some(true)
                })
        });

        assert!(
            bass_link,
            "{name} should link a control to bass (the autopilot contract)"
        );
        assert!(
            bpm_tween_link,
            "{name} should link a control to a BPM tween (the autopilot contract)"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

/// defaultScenes.spec.js:686-729: a pre-versioning install whose `scenes.json`
/// only has `defaultsSeeded: true` hydrates as defaults version 1 with the
/// original six Scene names, so the next seed only adds the newer batches.
#[test]
fn hydrates_legacy_files_that_only_have_defaults_seeded() {
    let dir = temp_dir();
    let state_path = dir.join("engine-state.json");
    let scenes_path = dir.join(SCENES_STORAGE_FILENAME);
    fs::write(
        &scenes_path,
        serde_json::to_vec(&json!({
            "version": 1,
            "defaultsSeeded": true,
            "scenes": [],
        }))
        .expect("legacy scenes.json"),
    )
    .expect("write legacy scenes.json");

    let mut store = Store::with_state_path(Some(&state_path));
    store
        .dispatch("scenes/hydrateScenes", json!({}))
        .expect("hydrate");

    let expected_names = get_default_scene_names_up_to(1);
    let state = store.scenes_state();

    assert!(state.defaults_seeded);
    assert_eq!(
        state.defaults_version, 1,
        "a legacy defaultsSeeded file is defaults version 1"
    );
    assert_eq!(state.seeded_default_scenes.len(), expected_names.len());
    assert!(
        state
            .seeded_default_scenes
            .contains(&"Mycelium Bloom".to_string())
    );
    assert!(
        !state
            .seeded_default_scenes
            .contains(&"Burning Mycelium Circle".to_string()),
        "the version-2 batch must not be claimed as seeded"
    );
    assert_eq!(DEFAULTS_VERSION, 20);

    let _ = fs::remove_dir_all(&dir);
}
