//! Scenes and performance-mode state for the engine store (WS-8, M5b).
//!
//! This module owns the Scene half of the store: the Scene list, the default
//! set, `scenes.json` persistence, `applyScene`, live-state capture and the
//! playlist import/export actions. The pure logic lives in `modv-engine`
//! ([`modv_engine::scenes_playlist`], [`modv_engine::default_scenes`],
//! [`modv_engine::scene_capture`], [`modv_engine::scene_rows`],
//! [`modv_engine::scene_timing`], [`modv_engine::scene_link_defaults`]); this
//! file is the store wiring the legacy worker performed in
//! `store/modules/scenes.js`.
//!
//! ## State (`getDefaultState`, `scenes.js:902-919`)
//!
//! [`ScenesState`] mirrors the Vuex `scenes` module state:
//! `scenes`, `defaultsSeeded`, `defaultsVersion`, `seededDefaultScenes` and
//! `performance`. The per-frame runtime fields (`transitioning`,
//! `activeRows`, `rowProgress`) are performance-mode runtime state and land
//! with M5c; `applyScene` emits the initial `SET_ACTIVE_ROWS` record so the UI
//! shows the applied grid.
//!
//! ## Persistence (`worker/index.worker.js:150-168`, WS-13)
//!
//! The worker debounced `scenes/persistScenes` 400 ms after any `scenes/*`
//! mutation. The engine writes synchronously after each Scene mutation for
//! M5b: the document is small (a few hundred kB at most) and a synchronous
//! write from the store keeps the on-disk list consistent with the state that
//! produced the deltas. The file is written atomically (tmp + rename) with the
//! WS-13 shape:
//!
//! ```json
//! {"version":1,"defaultsSeeded":true,"defaultsVersion":12,
//!  "seededDefaultScenes":["Name"],"scenes":[...],"performance":{...}}
//! ```
//!
//! `performance.currentSceneId` is a runtime value and never persisted
//! (`writeStoredScenes`, `scenes.js:1660-1683`).
//!
//! The path resolves from the engine's `--state` path: `<state parent>/scenes.json`
//! (the real media-manager/userData wiring is M7). A store constructed
//! without a state path (unit tests, the headless render path) keeps Scenes in
//! memory only.
//!
//! ## `applyScene` (`scenes.js:2529-2815`)
//!
//! Groups and Modules are rebuilt with **fresh ids** so the previous Groups
//! can be crossfaded out. The crossfade style comes from
//! `pickTransitionName`: `None` applies immediately, `Fade` keeps the outgoing
//! Scene-stamped Groups in the layer stack for `transitionSeconds` and fades
//! their alphas against the incoming set (the Fade branch of `applySceneNow`),
//! and a native shader effect keeps them at full alpha but hidden while
//! [`crate::scene_transition`] runs over the captured outgoing frame. Groups
//! that belong to no Scene survive every path. Row positions follow the Scene
//! document (`sceneRow`, named rows resolved through
//! [`modv_engine::scene_rows::assign_scene_row_indexes`]) and `null` row holes
//! leave the live row untouched.
//!
//! ## Known simplifications (documented, not silent)
//!
//! - **Only the six native transitions are available.** Custom ISF transition
//!   modules are filtered out of the allowed list by
//!   [`modv_engine::scene_transition_pick::pick_transition_name`], so a
//!   missing name fades instead — the JS `fallbackToFade` behaviour. A shader
//!   transition whose outgoing frame cannot be captured also falls back to
//!   Fade (`Store::fall_back_scene_transition_to_fade`).
//! - **Performance runtime lives in [`crate::performance`].**
//!   `scenes/startPerformance`, `scenes/stopPerformance`, `scenes/tick`,
//!   `scenes/nextScene` and the row-level apply are wired there; partial-Scene
//!   previews are served by `preview/scene` (see [`crate::scene_preview`]).
//! - **Default-scene migrations run before seeding.** Seeded Scenes retired
//!   between the persisted defaults version and the current one are renamed or
//!   removed by
//!   [`modv_engine::default_scene_migrations::ensure_default_scene_migration`]
//!   first, so newly introduced definitions neither duplicate a renamed Scene
//!   nor resurrect a removed one.
//! - **Scene preview frames are rendered by `preview/scene`**, not here; the
//!   store only exposes the Scene document and bumps the list revision that
//!   invalidates the preview cache.
//! - **Meyda smoothing ids are minted at build time.** `createBassLink` asks
//!   `meyda/getSmoothingId` for the feature smoothing argument
//!   (`scenes.js:758-789`), so a default Scene link that declares `smoothing`
//!   is built with `args: [feature, smoothingId, amount]` and the minted id is
//!   seeded in the store's smoothing registry (`addSmoothingId` starts it at
//!   `0`). Links without a declared smoothing keep `args: [feature]`.
//! - **Hydration keeps the full performance config.** The JS hydrate commits
//!   only the normalised timing keys (`scenes.js:2839-2858`), which drops
//!   `rowNames`/`order`/`allowedTransitions` on every restart. The engine
//!   persists and hydrates the whole config so the named grid survives.
//!   `enabled` hydrates too — a legacy `autopilot` left running resumes
//!   performance mode like the JS state — while `currentSceneId` is runtime
//!   state and the live value wins over the stored one.
//! - **Whole-Scene applies restore the Scene's MIDI state and focused
//!   input.** `restoreSceneMidi` and the focused-input tail of
//!   `restoreSceneInputs` run only for a whole-Scene apply; a row/scoped apply
//!   leaves both alone (`scenes.js` `scoped` early-return).
//!
//! Line numbers refer to the tree at commit `8a512b6e` (branch `rust-hybrid`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use modv_engine::default_scenes::{
    DEFAULTS_VERSION, LEGACY_DEFAULTS_VERSION, SceneDocument, get_default_scene_names_up_to,
    scene_key,
};
use modv_engine::engine_state::{
    GALLERY_GROUP_NAME, GROUP_ENABLED, Group, InheritFrom, ModuleInstance,
};
use modv_engine::inputs::{Input, InputLink};
use modv_engine::preset::create_preset_from_state;
use modv_engine::props::coerce_prop_value;
use modv_engine::scene_capture::{capture_live_state, collect_scene_groups, scene_has_groups};
use modv_engine::scene_cells::{clear_scene_cell_modules, copy_scene_cell, merge_scene_cell};
use modv_engine::scene_columns::{EmptySceneOptions, create_empty_scene, move_scene_to_index};
use modv_engine::scene_link_defaults::{
    LinkMode, default_link_for_input, default_link_tween, get_bass_link_range, get_bpm_link_range,
    get_feature_smoothing, is_auto_link_excluded,
};
use modv_engine::scene_rows::get_row_label;
use modv_engine::scene_rows::{
    assign_scene_row_indexes, disabled_cells_json, get_row_count, get_row_group_index,
    is_row_cell_empty, matches_row_name, normalize_disabled_cells_for_scenes, normalize_row_names,
    row_family_is_shared, set_scene_cell_disabled, strict_matches_row_name,
};
use modv_engine::scene_timing::{
    TimingUnit, normalize_scene_timing_values, normalize_timing_config,
    normalize_transition_duration, seconds_to_unit, to_finite_number, unit_to_seconds,
};
use modv_engine::scene_transition_pick::{FADE_TRANSITION, NONE_TRANSITION};
use modv_engine::scenes_playlist::{
    ImportMode, export_playlist, generate_scene_id, import_playlist_with,
    normalize_performance_data, normalize_transition_list,
};

use crate::module_registry::ModuleRegistry;
use crate::store_ops::{Store, StoreError, json_value, module_json};

/// The Scene-list file inside the engine's state directory (`scenes.json`,
/// `scenes.js:1645`).
pub const SCENES_STORAGE_FILENAME: &str = "scenes.json";

/// Version stamped into `scenes.json` (`writeStoredScenes`, `scenes.js:1673`).
pub const SCENES_STORAGE_VERSION: i64 = 1;

/// The six Group controls that own an input (`groups.js:174-197`).
///
/// Shared by the Scene machinery (live-group rebuilds) and
/// [`crate::store_ops::Store::create_group`].
pub(crate) const GROUP_CONTROL_INPUTS: &[(&str, &str)] = &[
    ("alpha", "alphaInputId"),
    ("enabled", "enabledInputId"),
    ("clearing", "clearingInputId"),
    ("inherit", "inheritInputId"),
    ("compositeOperation", "compositeOperationInputId"),
    ("pipeline", "pipelineInputId"),
];

/// The three Module meta controls that own an input (`modules.js:291-318`).
pub(crate) const MODULE_META_INPUTS: &[&str] = &["alpha", "enabled", "compositeOperation"];

/// Module props preferred for the automatic bass link
/// (`BASS_PROP_PREFERENCE`, `scenes.js:157-178`).
const BASS_PROP_PREFERENCE: &[&str] = &[
    "intensity",
    "amount",
    "brightness",
    "colorize",
    "bleedAmount",
    "grainLevel",
    "level",
    "strength",
    "saturation",
    "softness",
    "blurAmount",
    "spacing",
    "line_width",
    "scale",
    "radius",
    "size",
    "strobeRate",
    "gain",
];

/// Module props preferred for the automatic BPM link
/// (`BPM_PROP_PREFERENCE`, `scenes.js:180-194`).
const BPM_PROP_PREFERENCE: &[&str] = &[
    "rotation",
    "rotationSpeed",
    "rotateSpeed",
    "angle",
    "hue",
    "shift",
    "offset",
    "phase",
    "speed",
    "rate",
    "count",
    "sides",
    "numColors",
    "width",
];

/// Group priority for the automatic bass link (`scenes.js:973-979`).
const LINK_GROUP_PRIORITY: &[&str] = &["Effects", "Foreground", "Background"];

/// Group priority for the automatic BPM link fallback (`scenes.js:1002-1008`).
const BPM_GROUP_PRIORITY: &[&str] = &["Foreground", "Background", "Effects"];

/// Group keys modelled by [`Group`]; everything else in a Scene snapshot is
/// preserved as an unknown key (`extra`), mirroring the JS shallow spread.
const GROUP_KNOWN_KEYS: &[&str] = &[
    "id",
    "name",
    "hidden",
    "modules",
    "enabled",
    "alpha",
    "inherit",
    "inheritFrom",
    "pipeline",
    "clearing",
    "compositeOperation",
    "sceneId",
    "sceneRow",
    "transitionGhost",
    "drawToCanvasId",
    "alphaInputId",
    "enabledInputId",
    "clearingInputId",
    "inheritInputId",
    "compositeOperationInputId",
    "pipelineInputId",
    "context",
    "writeToSwap",
    "readFromSwap",
];

/// The `scenes` store state (`getDefaultState`, `scenes.js:902-919`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScenesState {
    /// Scene documents, in grid order.
    pub scenes: Vec<Value>,
    /// The defaults version the install last seeded (`defaultsVersion`).
    pub defaults_version: i64,
    /// Whether the default set has been seeded at least once.
    pub defaults_seeded: bool,
    /// Names of the default Scenes this install seeded (so deleting one does
    /// not resurrect it).
    pub seeded_default_scenes: Vec<String>,
    /// The performance config (`minSeconds`, `maxSeconds`,
    /// `transitionSeconds`, `allowedTransitions`, `order`, `rowNames`,
    /// `enabled`, `currentSceneId`).
    pub performance: Value,
}

impl Default for ScenesState {
    fn default() -> Self {
        Self {
            scenes: Vec::new(),
            defaults_version: 0,
            defaults_seeded: false,
            seeded_default_scenes: Vec::new(),
            performance: default_performance(),
        }
    }
}

/// The default performance config (`getDefaultState`, `scenes.js:902-919`).
pub fn default_performance() -> Value {
    json!({
        "enabled": false,
        "currentSceneId": Value::Null,
        "timingUnit": "seconds",
        "minSeconds": 8,
        "maxSeconds": 24,
        "transitionSeconds": 2,
        "allowedTransitions": [],
        "order": "in-order",
        "rowNames": [],
    })
}

/// A parsed `scenes.json` document (before it is committed to the state).
#[derive(Clone, Debug, PartialEq)]
struct ParsedStoredScenes {
    scenes: Vec<Value>,
    performance: Value,
    defaults_version: i64,
    defaults_seeded: bool,
    seeded_default_scenes: Vec<String>,
}

/// What a row-level Scene apply did, so the performance runtime can record the
/// crossfade (`applySceneRowNow`, `scenes.js:2247-2523`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowApplyOutcome {
    /// The incoming (live) Group id.
    pub group_id: String,
    /// The outgoing Group kept alive as a fading ghost, when the row faded.
    pub ghost_group_id: Option<String>,
    /// The ghost's starting alpha.
    pub from_alpha: f64,
    /// The incoming Group's target alpha.
    pub target_alpha: f64,
    /// Crossfade duration (`transitionSeconds`).
    pub duration: f64,
}

impl Store {
    /// The Scene state (tests and diagnostics).
    #[allow(dead_code)]
    pub fn scenes_state(&self) -> &ScenesState {
        &self.scenes
    }

    /// The Scene document and row-name mapping a `preview/scene` request
    /// needs.
    ///
    /// Looks the Scene up by id in the store's list (so unsaved/edited Scenes
    /// are visible) and returns it with the current `performance.rowNames`
    /// mapping, which is what resolves named rows for the cell preview.
    /// `None` when no Scene has that id; the caller answers
    /// `scene_not_found`.
    pub fn scene_preview_document(&self, scene_id: &str) -> Option<(Value, Vec<String>)> {
        let scene = self
            .scenes
            .scenes
            .iter()
            .find(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id))?
            .clone();
        let row_names = normalize_row_names(self.scenes.performance.get("rowNames"));
        Some((scene, row_names))
    }

    /// The resolved `scenes.json` path, when the store has a state path.
    #[allow(dead_code)]
    pub fn scenes_path(&self) -> Option<&Path> {
        self.scenes_path.as_deref()
    }

    /// `scenes/hydrateScenes` (`scenes.js:2835-2899`).
    ///
    /// Reads `scenes.json` when present, validates and loads the Scene list,
    /// performance config and seeding metadata. Unknown keys are ignored at
    /// both the document and Scene level. A missing file leaves the state
    /// empty so `ensureDefaultScenes` seeds the defaults.
    pub fn hydrate_scenes(&mut self) -> Result<Value, StoreError> {
        let Some(path) = self.scenes_path.clone() else {
            return Ok(Value::Null);
        };
        let stored = match read_json_file(&path) {
            Ok(Some(stored)) => stored,
            Ok(None) => return Ok(Value::Null),
            Err(error) => {
                self.warn_once(
                    "scenes:hydrate-read".to_string(),
                    &format!(
                        "engine: WARN could not read the saved Scene list at {}: {error}",
                        path.display()
                    ),
                );
                return Ok(Value::Null);
            }
        };
        let Some(parsed) = parse_stored_scenes(&stored) else {
            self.warn_once(
                "scenes:hydrate-invalid".to_string(),
                &format!(
                    "engine: WARN could not read the saved Scene list at {}",
                    path.display()
                ),
            );
            return Ok(Value::Null);
        };

        self.scenes.scenes = parsed.scenes;
        let live_current_scene = self
            .scenes
            .performance
            .get("currentSceneId")
            .cloned()
            .unwrap_or(Value::Null);
        self.scenes.performance = parsed.performance;
        if let Some(object) = self.scenes.performance.as_object_mut() {
            // Runtime state: the JS hydrate takes `currentSceneId` from the
            // live module state, never from the stored document.
            object.insert("currentSceneId".to_string(), live_current_scene);
        }
        // `enabled` is state, not runtime-only, in the JS store: the tick
        // reads `state.performance.enabled`, so a legacy autopilot that was
        // left on resumes after hydration. Keep the engine runtime in step.
        self.performance.enabled = self.performance_enabled();
        self.scenes.defaults_version = parsed.defaults_version;
        self.scenes.defaults_seeded = parsed.defaults_seeded;
        self.scenes.seeded_default_scenes = parsed.seeded_default_scenes;
        self.touch_scene_list();

        self.emit_scene_list();
        self.emit_performance();
        self.emit_defaults_metadata();
        Ok(json!({
            "scenes": self.scenes.scenes.len(),
            "defaultsVersion": self.scenes.defaults_version,
            "defaultsSeeded": self.scenes.defaults_seeded,
        }))
    }

    /// `scenes/loadScenesFromDisk` (`scenes.js:3102-3164`).
    ///
    /// Reloads `scenes.json` and merges it into the current list by id
    /// (imported Scenes win). The JS helper merges per-Scene files using their
    /// mtimes through the media manager; the engine has no media manager until
    /// M7, so the authoritative `scenes.json` is the merge source here.
    pub fn load_scenes_from_disk(&mut self) -> Result<Value, StoreError> {
        let Some(path) = self.scenes_path.clone() else {
            return Ok(Value::Null);
        };
        let Some(stored) = read_json_file(&path).unwrap_or_else(|error| {
            eprintln!("engine: WARN could not read the saved Scene list: {error}");
            None
        }) else {
            return Ok(Value::Null);
        };
        let Some(parsed) = parse_stored_scenes(&stored) else {
            return Ok(Value::Null);
        };

        let mut imported = 0usize;
        for scene in parsed.scenes {
            let Some(id) = scene.get("id").and_then(Value::as_str) else {
                continue;
            };
            match self
                .scenes
                .scenes
                .iter()
                .position(|existing| existing.get("id").and_then(Value::as_str) == Some(id))
            {
                Some(index) => self.scenes.scenes[index] = scene.clone(),
                None => self.scenes.scenes.push(scene.clone()),
            }
            imported += 1;
            self.deltas.push("scenes/ADD_SCENE", scene);
        }

        // The JS `loadScenesFromDisk` only merges Scenes; performance state is
        // untouched. Keep the live runtime keys over the parsed file so a
        // reload cannot silently start/stop performance mode.
        let live_performance = self.scenes.performance.clone();
        self.scenes.performance = parsed.performance;
        if let Some(object) = self.scenes.performance.as_object_mut() {
            for key in ["enabled", "currentSceneId"] {
                object.insert(
                    key.to_string(),
                    live_performance.get(key).cloned().unwrap_or(Value::Null),
                );
            }
        }
        self.scenes.defaults_version = parsed.defaults_version;
        self.scenes.defaults_seeded = parsed.defaults_seeded;
        self.scenes.seeded_default_scenes = parsed.seeded_default_scenes;
        self.touch_scene_list();

        self.emit_performance();
        self.emit_defaults_metadata();
        Ok(json!({ "imported": imported }))
    }

    /// `scenes/ensureDefaultScenes` (`scenes.js:2904-3090`).
    ///
    /// Seeds the ready-made music-reactive Scenes on first run and appends any
    /// defaults introduced since the last seeding pass. Seeding is versioned
    /// and name-matched, so an existing install receives later batches without
    /// duplicates and a deleted default does not come back. Each Scene
    /// document is built like `buildSceneSnapshot` (`scenes.js:1689-1835`) but
    /// without touching the live layer stack.
    pub fn ensure_default_scenes(&mut self) -> Result<Value, StoreError> {
        let current_version = self.scenes.defaults_version.max(0);
        let mut seeded = normalize_default_scene_name_list(&self.scenes.seeded_default_scenes);

        // Rebuild the Scenes whose default definitions changed in this
        // version (`getDefaultScenesToReplace` → `ADD_SCENE`,
        // `scenes.js:2918-2951`). Only the payload keys are swapped; the
        // existing Scene keeps its id, name, timing and enabled flag, so the
        // active Scene and user metadata survive. Renames/removals stay with
        // `ensure_default_scene_migration` below, so only `replacements` are
        // consumed here.
        let refresh = modv_engine::default_scenes::get_default_scenes_to_replace(
            current_version,
            &self.scenes.scenes,
            None,
            Some(&seeded),
        );
        let mut refreshed = 0usize;

        for replacement in refresh.replacements {
            let mut skipped = 0usize;
            let built = build_default_scene_document(
                &self.registry,
                replacement.definition,
                &self.state.midi,
                &mut skipped,
            );
            self.register_link_smoothing(&built.smoothing_ids);
            let Some(existing) = self
                .scenes
                .scenes
                .iter_mut()
                .find(|scene| scene.get("id") == Some(&replacement.id))
            else {
                continue;
            };
            let (Some(existing_object), Some(built_object)) =
                (existing.as_object_mut(), built.document.as_object())
            else {
                continue;
            };

            for key in ["groups", "modules", "inputs", "expressions", "tweens"] {
                if let Some(value) = built_object.get(key) {
                    existing_object.insert(key.to_string(), value.clone());
                }
            }

            refreshed += 1;
        }

        if refreshed > 0 {
            eprintln!(
                "engine: INFO refreshed {refreshed} default Scene(s) to version {DEFAULTS_VERSION}"
            );
        }

        // Rename/remove seeded Scenes that the defaults retired between the
        // persisted version and this one *before* seeding, so the new
        // definitions neither duplicate a renamed Scene nor resurrect a
        // removed one. The plan's `seeds` are the still-missing defaults.
        let plan = modv_engine::default_scene_migrations::ensure_default_scene_migration(
            current_version,
            DEFAULTS_VERSION,
            &mut self.scenes.scenes,
            &mut seeded,
        );
        if !plan.report.renamed.is_empty() || !plan.report.removed.is_empty() {
            eprintln!(
                "engine: INFO default Scene migration: {} renamed, {} removed, {} skipped",
                plan.report.renamed.len(),
                plan.report.removed.len(),
                plan.report.skipped.len()
            );
        }

        let seeded_keys: HashSet<String> = seeded.iter().map(|name| scene_key(name)).collect();
        let existing_names: Vec<String> = self
            .scenes
            .scenes
            .iter()
            .filter_map(|scene| scene.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        let existing_keys: HashSet<String> =
            existing_names.iter().map(|name| scene_key(name)).collect();

        let definitions = plan.seeds;
        let mut added = Vec::new();
        let mut skipped_modules = 0usize;

        for definition in definitions {
            let mut skipped = 0usize;
            let built = build_default_scene_document(
                &self.registry,
                definition,
                &self.state.midi,
                &mut skipped,
            );
            self.register_link_smoothing(&built.smoothing_ids);
            skipped_modules += skipped;
            added.push(definition.name.clone());
            self.scenes.scenes.push(built.document);
        }

        // `markSeeded` for every default whose name already exists in the list
        // (`scenes.js:3076-3087`): a user Scene sharing a default's name counts
        // as seeded so deleting it later does not resurrect the default.
        for definition in modv_engine::default_scenes::default_scenes() {
            let key = scene_key(&definition.name);
            if existing_keys.contains(&key) && !seeded_keys.contains(&key) {
                seeded.push(definition.name.clone());
            }
        }
        for name in &added {
            if !seeded
                .iter()
                .any(|existing| scene_key(existing) == scene_key(name))
            {
                seeded.push(name.clone());
            }
        }

        self.scenes.seeded_default_scenes = seeded;
        self.scenes.defaults_seeded = true;
        self.scenes.defaults_version = DEFAULTS_VERSION.max(current_version);
        self.touch_scene_list();

        self.emit_scene_list();
        self.emit_defaults_metadata();
        self.persist_best_effort();

        if !added.is_empty() && skipped_modules > 0 {
            eprintln!(
                "engine: INFO seeded {} default Scene(s); skipped {} Module definition(s) not in the M5 registry",
                added.len(),
                skipped_modules
            );
        }

        Ok(json!({
            "seeded": added,
            "defaultsVersion": self.scenes.defaults_version,
        }))
    }

    /// `scenes/persistScenes` (`scenes.js:3093-3095`).
    ///
    /// Writes the WS-13 document atomically. A store without a state path
    /// (headless/tests) is a no-op.
    pub fn persist_scenes(&self) -> io::Result<bool> {
        let Some(path) = self.scenes_path.as_deref() else {
            return Ok(false);
        };
        let mut performance = self.scenes.performance.clone();
        if let Some(object) = performance.as_object_mut() {
            // `writeStoredScenes`: runtime state is not persisted.
            object.remove("currentSceneId");
            if object.get("disabledCells").is_some() {
                let row_names = normalize_row_names(object.get("rowNames"));
                let disabled = normalize_disabled_cells_for_scenes(
                    object.get("disabledCells"),
                    &self.scenes.scenes,
                    &row_names,
                );
                object.insert("disabledCells".to_string(), disabled_cells_json(&disabled));
            }
        }
        let payload = json!({
            "version": SCENES_STORAGE_VERSION,
            "defaultsSeeded": self.scenes.defaults_seeded,
            "defaultsVersion": self.scenes.defaults_version,
            "seededDefaultScenes": self.scenes.seeded_default_scenes,
            "scenes": self.scenes.scenes,
            "performance": performance,
        });
        let bytes = serde_json::to_vec(&payload).map_err(io::Error::other)?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, path)?;
        Ok(true)
    }

    /// `scenes/applyScene` (`scenes.js:2538-2815`).
    ///
    /// Rebuilds the live layer stack from the Scene document: one fresh Group
    /// per non-`null` row with fresh Module instances, Scene stamps, mapped
    /// inherit targets and the saved input links.
    pub fn apply_scene_action(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let action = "scenes/applyScene";
        let scene_id = required_str(params, action, "sceneId")?.to_string();
        let Some(raw) = self
            .scenes
            .scenes
            .iter()
            .find(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id.as_str()))
            .cloned()
        else {
            self.warn_once(
                format!("scene-apply-missing:{scene_id}"),
                &format!("engine: WARN {action}: no Scene with id {scene_id}"),
            );
            return Ok(Value::Null);
        };
        let Some(scene) = modv_engine::scenes_playlist::normalize_scene_document(&raw) else {
            return Ok(Value::Null);
        };

        let row_names = normalize_row_names(self.scenes.performance.get("rowNames"));
        // A UI editor may still name an input this apply is about to replace;
        // remember what each live input drove so its Save can be retargeted.
        let retired_inputs = self.capture_input_identities();
        let group_snapshots = scene
            .get("groups")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let module_snapshots = scene
            .get("modules")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let row_indexes = assign_scene_row_indexes(&group_snapshots, &row_names);

        // `applySceneNow` finishes any in-flight transition first, then keeps
        // the outgoing Groups alive when the change crossfades
        // (`scenes.js:2538-2568`).
        self.complete_all_row_transitions();
        self.complete_whole_scene_transition();

        let old_groups: Vec<Group> = self
            .state
            .groups
            .iter()
            .filter(|group| {
                // Every Scene-stamped Group is managed by the Scene system,
                // including rows applied one-by-one in performance mode.
                // Groups that belong to no Scene (a hand-built "Free" layer)
                // survive, matching the M5b replacement rule.
                group.name != GALLERY_GROUP_NAME
                    && group.transition_ghost != Some(true)
                    && group.scene_id.is_some()
            })
            .cloned()
            .collect();
        let old_ids: HashSet<String> = old_groups.iter().map(|group| group.id.clone()).collect();
        self.state
            .groups
            .retain(|group| !old_ids.contains(&group.id));

        let mut input_map: HashMap<String, String> = HashMap::new();
        let mut created: Vec<CreatedGroup> = Vec::new();

        for (index, snapshot) in group_snapshots.iter().enumerate() {
            let Some(object) = snapshot.as_object() else {
                // A row hole (`null`) keeps the live row untouched
                // (`scenes.js:2570-2574`).
                continue;
            };
            let row = row_indexes.get(index).copied().flatten().unwrap_or(index);

            let mut group = self.build_live_group(object, &scene_id, row, &mut input_map);
            let empty_modules = group.modules.clone();
            group.modules.clear();

            // The mirror adds the Group first and its Modules one by one
            // (`ADD_GROUP` then `ADD_MODULE_TO_GROUP`), exactly like
            // `groups/createGroup` + `groups/addModuleToGroup`.
            let mut group_delta = json_value(&group);
            if let Value::Object(object) = &mut group_delta {
                object.insert("modules".to_string(), json!(empty_modules));
            }
            self.deltas
                .push("groups/ADD_GROUP", json!({ "group": group_delta }));

            let module_keys = object
                .get("modules")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for key in module_keys {
                let Some(key) = key.as_str() else {
                    continue;
                };
                let Some(module_snapshot) = module_snapshots.get(key) else {
                    continue;
                };
                if let Some(module_id) = self.build_live_module(module_snapshot, &mut input_map) {
                    group.modules.push(module_id.clone());
                    self.deltas.push(
                        "groups/ADD_MODULE_TO_GROUP",
                        json!({ "moduleId": module_id, "groupId": group.id }),
                    );
                }
            }

            let old_group_id = object.get("id").and_then(Value::as_str).map(str::to_string);

            created.push(CreatedGroup {
                row,
                old_group_id,
                group,
            });
        }

        // Insert in row order, preserving the live row layout around the
        // Gallery group and any Groups that belong to no Scene.
        created.sort_by_key(|created| created.row);
        let id_map: HashMap<String, String> = created
            .iter()
            .filter_map(|created| {
                created
                    .old_group_id
                    .clone()
                    .map(|old| (old, created.group.id.clone()))
            })
            .collect();
        let created_alphas: Vec<(String, f64)> = created
            .iter()
            .map(|created| (created.group.id.clone(), created.group.alpha))
            .collect();
        for created_group in created {
            self.insert_group_at_row(created_group.group);
        }

        // Remap `inheritFrom` now that every Group has its final id
        // (`scenes.js:2679-2703`).
        self.remap_scene_inherit(&group_snapshots, &id_map);

        // A manual change picks its transition style (`applySceneNow`,
        // `scenes.js:2714`): `None` switches instantly, `Fade` crossfades the
        // outgoing Groups' alphas, and a shader effect (one of the six natives
        // or a registered ISF transition) keeps both Scenes at full opacity
        // while the compositor runs the effect over the captured outgoing
        // frame. The outgoing Groups are hidden, not faded, for the shader
        // path, so only one animation runs.
        let transition_name = self.pick_scene_transition_name();
        let effect =
            crate::scene_transition::TransitionEffect::from_name(&transition_name, &self.registry);
        // A name that is neither `Fade`, `None` nor a runnable effect (an
        // unknown/custom transition, or a registered module that is a filter
        // rather than a transition) degrades to Fade, the JS `fallbackToFade`
        // behaviour. The picker's available list normally prevents this, but
        // an explicit request must never switch instantly just because its
        // effect is missing.
        let transition_name = if effect.is_none()
            && transition_name != FADE_TRANSITION
            && transition_name != NONE_TRANSITION
        {
            FADE_TRANSITION.to_string()
        } else {
            transition_name
        };
        let has_old_groups = !old_groups.is_empty();
        let shader = effect.is_some() && has_old_groups;
        let fade = transition_name == FADE_TRANSITION && has_old_groups;
        if fade || shader {
            // The outgoing Groups stay in the layer stack for the crossfade.
            // They are inserted *before* the created Groups, so after
            // `emit_group_order`'s stable row sort each row draws its outgoing
            // Group first and its incoming Group on top of it — the same shape
            // as the JS `orderRowGroups` order (outgoing below, incoming above
            // within a row). R-29: the outgoing Scene used to stay on top
            // because the created Groups were inserted first.
            let anchor = created_alphas
                .first()
                .map(|(group_id, _)| group_id.as_str())
                .and_then(|group_id| {
                    self.state
                        .groups
                        .iter()
                        .position(|group| group.id == group_id)
                })
                .unwrap_or(self.state.groups.len());
            let mut from = Vec::new();
            for (insert_at, mut group) in (anchor..).zip(old_groups) {
                // The ghost reuses the outgoing Modules but not the Group
                // control inputs, which belong to the replaced Group.
                for (_, key) in GROUP_CONTROL_INPUTS {
                    if let Some(input_id) = group_input_id(&group, key) {
                        self.remove_input(&input_id);
                    }
                    set_group_input_id(&mut group, key, None);
                }
                from.push((group.id.clone(), group.alpha));
                self.state.groups.insert(insert_at, group);
            }
            if !shader {
                for (group_id, _) in &created_alphas {
                    if let Some(group) = self.state.find_group_mut(group_id) {
                        group.alpha = 0.0;
                    }
                    self.deltas.push(
                        "groups/UPDATE_GROUP",
                        json!({ "groupId": group_id, "data": { "alpha": 0.0 } }),
                    );
                }
            }
            self.emit_group_order();
            self.begin_whole_scene_transition(from, created_alphas, transition_name, effect);
        } else {
            for group in old_groups {
                self.release_group(group);
            }
            // The final layer stack, authoritative for the mirror (the JS
            // `groups/orderByIds` action commits `REPLACE_GROUPS`).
            self.emit_group_order();
        }

        self.restore_scene_inputs(&scene, &input_map);
        self.restore_scene_focused_input(&scene, &input_map);
        self.restore_scene_expressions(&scene, &input_map, None);
        self.restore_scene_midi(&scene);
        self.applied_scene_id = Some(scene_id.clone());
        self.set_current_scene(&scene_id);

        let active_rows = self.scene_active_rows(&scene_id);
        // `SET_ACTIVE_ROWS(state, activeRows)` takes the map directly
        // (`scenes.js:4156-4158`).
        self.deltas.push("scenes/SET_ACTIVE_ROWS", active_rows);

        // A whole-scene change rebuilds every row; re-seed the dwell runtimes
        // with the new Groups so the next tick does not treat them as stale
        // (`resetAllRowRuntime`, `scenes.js:2792-2799`).
        if self.performance.enabled {
            let row_count = self.performance_row_count();
            for row in 0..row_count {
                let live = self.find_live_row_group(row);
                let scene_id_for_row = live
                    .as_ref()
                    .and_then(|group| group.scene_id.clone())
                    .or_else(|| Some(scene_id.clone()));
                let group_id = live.as_ref().map(|group| group.id.clone());
                self.reset_row_runtime(row, scene_id_for_row, group_id);
            }
        }

        self.retire_replaced_inputs(retired_inputs);
        self.persist_best_effort();
        Ok(json!({ "sceneId": scene_id }))
    }

    /// The row-level half of `applyScene` (`applySceneRowNow`,
    /// `scenes.js:2250-2525`): replaces the Group at one row only, keeping the
    /// other rows untouched, and optionally keeps the outgoing Group alive as
    /// a `transitionGhost` for the crossfade.
    pub(crate) fn apply_scene_row_now(
        &mut self,
        scene_id: &str,
        row: usize,
        fade: bool,
    ) -> Option<RowApplyOutcome> {
        let raw = self
            .scenes
            .scenes
            .iter()
            .find(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id))?
            .clone();
        let scene = modv_engine::scenes_playlist::normalize_scene_document(&raw)?;
        let row_names = normalize_row_names(self.scenes.performance.get("rowNames"));
        if is_row_cell_empty(&scene, row, &row_names) {
            return None;
        }
        let snapshot_index = get_row_group_index(&scene, row, &row_names)?;
        let snapshot = scene
            .get("groups")
            .and_then(Value::as_array)?
            .get(snapshot_index)?
            .as_object()?
            .clone();

        // A link editor opened on this row still names the input ids the row
        // apply replaces; keep the control identities for the retarget path.
        let retired_inputs = self.capture_input_identities();

        self.complete_whole_scene_transition();
        self.complete_row_transition(row);

        let old_group = self.find_live_row_group(row);
        let old_modules = old_group
            .as_ref()
            .map(|group| group.modules.clone())
            .unwrap_or_default();
        let from_alpha = old_group.as_ref().map(|group| group.alpha).unwrap_or(0.0);
        let target_alpha = snapshot.get("alpha").and_then(Value::as_f64).unwrap_or(1.0);
        let fade = fade && old_group.is_some() && !old_modules.is_empty() && target_alpha > 0.0;
        let duration =
            normalize_transition_duration(self.scenes.performance.get("transitionSeconds"));

        // Expressions on inputs that this row apply drops must go with them
        // (`scenes.js:2429-2448`): the old Group's control inputs always, and
        // the old Modules' inputs when no ghost keeps those Modules alive.
        let mut stale_input_ids: HashSet<String> = HashSet::new();
        if let Some(group) = &old_group {
            for (_, key) in GROUP_CONTROL_INPUTS {
                if let Some(input_id) = group_input_id(group, key) {
                    stale_input_ids.insert(input_id);
                }
            }
        }
        if !fade {
            for module_id in &old_modules {
                if let Some(module) = self.state.modules.get(module_id) {
                    if let Some(props) = module.dollar_props.as_object() {
                        for descriptor in props.values() {
                            if let Some(input_id) = descriptor.get("id").and_then(Value::as_str) {
                                stale_input_ids.insert(input_id.to_string());
                            }
                        }
                    }
                    for key in MODULE_META_INPUTS {
                        if let Some(input_id) = module
                            .meta
                            .get(format!("{key}InputId"))
                            .and_then(Value::as_str)
                        {
                            stale_input_ids.insert(input_id.to_string());
                        }
                    }
                }
            }
        }

        let mut input_map: HashMap<String, String> = HashMap::new();
        let mut new_group = self.build_live_group(&snapshot, scene_id, row, &mut input_map);
        new_group.inherit_from = self.remap_row_inherit(&scene, &snapshot);

        let module_snapshots = scene
            .get("modules")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let mut new_modules = Vec::new();
        let module_keys = snapshot
            .get("modules")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for key in module_keys {
            let Some(key) = key.as_str() else {
                continue;
            };
            let Some(module_snapshot) = module_snapshots.get(key) else {
                continue;
            };
            if let Some(module_id) = self.build_live_module(module_snapshot, &mut input_map) {
                new_modules.push(module_id);
            }
        }

        let group_id = new_group.id.clone();
        let ghost_group_id = if let Some(old) = old_group {
            let old_index = self
                .state
                .groups
                .iter()
                .position(|group| group.id == old.id)?;

            // The replaced Group's control inputs are stale; the incoming
            // Group owns fresh ones and the ghost needs none.
            for (_, key) in GROUP_CONTROL_INPUTS {
                if let Some(input_id) = group_input_id(&old, key) {
                    self.remove_input(&input_id);
                }
            }
            self.deltas
                .push("groups/REMOVE_GROUP", json!({ "id": old.id }));

            new_group.modules = new_modules.clone();
            new_group.alpha = if fade { 0.0 } else { target_alpha };
            self.deltas.push(
                "groups/ADD_GROUP",
                json!({ "group": json_value(&new_group) }),
            );
            self.state.groups[old_index] = new_group.clone();

            if fade {
                let ghost = Group {
                    id: crate::ids::uuid_v4(),
                    name: old.name.clone(),
                    hidden: old.hidden,
                    modules: old.modules.clone(),
                    enabled: old.enabled,
                    alpha: from_alpha,
                    inherit: old.inherit,
                    inherit_from: old.inherit_from.clone(),
                    pipeline: old.pipeline,
                    clearing: old.clearing,
                    composite_operation: old.composite_operation.clone(),
                    scene_id: old.scene_id.clone(),
                    scene_row: Some(row),
                    transition_ghost: Some(true),
                    draw_to_canvas_id: old.draw_to_canvas_id.clone(),
                    alpha_input_id: None,
                    enabled_input_id: None,
                    clearing_input_id: None,
                    inherit_input_id: None,
                    composite_operation_input_id: None,
                    pipeline_input_id: None,
                    extra: old.extra.clone(),
                };
                let ghost_id = ghost.id.clone();
                self.state.groups.insert(old_index + 1, ghost.clone());
                self.deltas
                    .push("groups/ADD_GROUP", json!({ "group": json_value(&ghost) }));
                Some(ghost_id)
            } else {
                None
            }
        } else {
            new_group.modules = new_modules.clone();
            new_group.alpha = target_alpha;
            self.insert_group_at_row(new_group.clone());
            self.deltas.push(
                "groups/ADD_GROUP",
                json!({ "group": json_value(&new_group) }),
            );
            None
        };

        // Without a ghost nothing references the outgoing Modules any more.
        if ghost_group_id.is_none() {
            for module_id in &old_modules {
                let still_used = self
                    .state
                    .groups
                    .iter()
                    .any(|group| group.modules.iter().any(|id| id == module_id));
                if !still_used {
                    self.drop_active_module(module_id);
                }
            }
        }

        self.restore_scene_inputs(&scene, &input_map);
        self.restore_scene_expressions(&scene, &input_map, Some(&stale_input_ids));

        // `groups/orderByIds` (`scenes.js:2515-2520`): the mirror's layer
        // stack follows the live row order, with the fading ghost before its
        // incoming Group so the incoming Group draws on top.
        self.emit_group_order();

        self.deltas.push(
            "scenes/SET_ACTIVE_ROW",
            json!({ "rowIndex": row, "sceneId": scene_id, "groupId": group_id }),
        );
        if row == 0 {
            self.set_current_scene(scene_id);
        }

        self.retire_replaced_inputs(retired_inputs);
        Some(RowApplyOutcome {
            group_id,
            ghost_group_id,
            from_alpha,
            target_alpha,
            duration,
        })
    }

    /// Map a Scene document's `inheritFrom` target onto the live Group at the
    /// target row (`scenes.js:2450-2480`, `sceneGroupIdMap`).
    fn remap_row_inherit(&self, scene: &Value, snapshot: &Map<String, Value>) -> InheritFrom {
        let Some(target) = snapshot.get("inheritFrom").and_then(Value::as_str) else {
            return InheritFrom::Previous(-1);
        };
        let Some(index) = scene
            .get("groups")
            .and_then(Value::as_array)
            .and_then(|groups| {
                groups
                    .iter()
                    .position(|group| group.get("id").and_then(Value::as_str) == Some(target))
            })
        else {
            return InheritFrom::Previous(-1);
        };
        self.find_live_row_group(index)
            .map(|group| InheritFrom::Group(group.id))
            .unwrap_or(InheritFrom::Previous(-1))
    }

    /// `scenes/clearSceneCell` (`scenes.js:3371-3430`).
    ///
    /// Empties the Group saved in a performance-grid cell, prunes its Module
    /// snapshots and removes those Modules from the live Group when the cell
    /// is the active row for this Scene.
    pub fn clear_scene_cell(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let Some(row) = params
            .get("rowIndex")
            .and_then(Value::as_i64)
            .filter(|row| *row >= 0)
            .map(|row| row as usize)
        else {
            return Ok(Value::Null);
        };
        let Some(index) =
            self.scenes.scenes.iter().position(|scene| {
                scene.get("id").and_then(Value::as_str) == Some(scene_id.as_str())
            })
        else {
            return Ok(Value::Null);
        };

        let row_names = self.row_names();
        let scene = self.scenes.scenes[index].clone();
        let Some(cleared) = clear_scene_cell_modules(&scene, row, &row_names) else {
            return Ok(Value::Null);
        };

        let live_group = self
            .state
            .groups
            .iter()
            .find(|group| {
                group.scene_id.as_deref() == Some(scene_id.as_str())
                    && group.scene_row == Some(row)
                    && group.transition_ghost != Some(true)
            })
            .cloned();

        let mut updated = scene;
        if let Value::Object(object) = &mut updated {
            object.insert("groups".to_string(), Value::Array(cleared.groups.clone()));
            object.insert(
                "modules".to_string(),
                Value::Object(cleared.modules.clone()),
            );
        }
        self.scenes.scenes[index] = updated.clone();

        if let Some(group) = live_group {
            // `clearSceneCell` removes every Module the **live** Group owns
            // (`liveGroup.modules`, `scenes.js:3405-3413`); the cleared cell's
            // ids belong to the saved document, not the live instances.
            let live_module_ids = group.modules.clone();
            for module_id in live_module_ids {
                self.dispatch(
                    "modules/removeActiveModule",
                    json!({ "moduleId": module_id }),
                )?;
            }
            // `groups/REPLACE_GROUP_MODULES { modules: [] }`
            // (`scenes.js:3415-3418`): without the rewrite the live Group keeps
            // ids it no longer owns, and the next cell capture writes a Group
            // whose module ids have no snapshots.
            self.dispatch(
                "groups/REPLACE_GROUP_MODULES",
                json!({ "groupId": group.id, "modules": [] }),
            )?;
            self.deltas
                .push("scenes/SET_ACTIVE_ROWS", self.live_scene_rows());
        }

        self.touch_scene_list();
        self.deltas.push("scenes/ADD_SCENE", updated.clone());
        self.persist_best_effort();
        Ok(updated)
    }

    /// `scenes/createSceneCellGroup` (`scenes.js:3432-3485`).
    ///
    /// Replaces the live Group in a performance row with a fresh empty Group
    /// stamped for the Scene, so an empty Scene cell can be filled by editing
    /// the live stack. Returns `{ groupId }` like the JS action.
    pub fn create_scene_cell_group(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        if self.performance_enabled() {
            return Ok(Value::Null);
        }
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let Some(row) = params
            .get("rowIndex")
            .and_then(Value::as_i64)
            .filter(|row| *row >= 0)
            .map(|row| row as usize)
        else {
            return Ok(Value::Null);
        };
        if !self
            .scenes
            .scenes
            .iter()
            .any(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id.as_str()))
        {
            return Ok(Value::Null);
        }

        self.complete_whole_scene_transition();
        self.complete_row_transition(row);

        if let Some(old_group) = self
            .find_live_row_group(row)
            .filter(|group| group.name != GALLERY_GROUP_NAME)
        {
            self.remove_live_group(&old_group.id);
        }

        let row_names = self.row_names();
        let name = get_row_label(&self.scenes.scenes, row, &row_names);
        let group_params = json!({
            "name": name,
            "enabled": GROUP_ENABLED,
            "sceneId": scene_id,
            "sceneRow": row,
        });
        let group = self.create_group(
            group_params
                .as_object()
                .expect("group params are an object"),
        )?;

        if row == 0 {
            self.set_current_scene(&scene_id);
        }
        self.emit_group_order();
        self.deltas
            .push("scenes/SET_ACTIVE_ROWS", self.live_scene_rows());
        self.persist_best_effort();
        Ok(json!({ "groupId": group["id"] }))
    }

    /// `scenes/copySceneCellToScene` (`scenes.js:3568-3735`).
    ///
    /// Ctrl+drag a grid cell onto another Scene: copies the Group and Module
    /// snapshots verbatim (ids included) plus the saved inputs, input links,
    /// expression assignments and tweens that belong to them. Rows the target
    /// does not define become `null` holes so row indexes stay aligned.
    pub fn copy_scene_cell_to_scene(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        let from_scene_id = params
            .get("fromSceneId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let to_scene_id = params
            .get("toSceneId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let (Some(from_row), Some(to_row)) = (
            params
                .get("fromRowIndex")
                .and_then(Value::as_i64)
                .filter(|row| *row >= 0)
                .map(|row| row as usize),
            params
                .get("toRowIndex")
                .and_then(Value::as_i64)
                .filter(|row| *row >= 0)
                .map(|row| row as usize),
        ) else {
            return Ok(Value::Null);
        };
        if from_scene_id == to_scene_id {
            return Ok(Value::Null);
        }

        let Some(from_scene) = self
            .scenes
            .scenes
            .iter()
            .find(|scene| scene.get("id").and_then(Value::as_str) == Some(from_scene_id.as_str()))
            .cloned()
        else {
            return Ok(Value::Null);
        };
        let Some(index) = self.scenes.scenes.iter().position(|scene| {
            scene.get("id").and_then(Value::as_str) == Some(to_scene_id.as_str())
        }) else {
            return Ok(Value::Null);
        };
        let to_scene = self.scenes.scenes[index].clone();
        let row_names = self.row_names();

        let Some(data) = copy_scene_cell(&from_scene, &to_scene, from_row, to_row, &row_names)
        else {
            return Ok(Value::Null);
        };

        let mut updated = to_scene;
        if let (Value::Object(object), Value::Object(data)) = (&mut updated, &data) {
            for key in ["groups", "modules", "inputs", "expressions", "tweens"] {
                if let Some(value) = data.get(key) {
                    object.insert(key.to_string(), value.clone());
                }
            }
        }
        self.scenes.scenes[index] = updated.clone();
        self.touch_scene_list();
        self.deltas.push("scenes/ADD_SCENE", updated.clone());
        self.persist_best_effort();
        Ok(updated)
    }

    /// `scenes/captureSceneRowToScene` (`scenes.js:3487-3567`).
    ///
    /// Writes the live Group of one row back into its Scene cell, merging the
    /// Module snapshots, inputs, input links, expressions and tweens. Only a
    /// Group stamped for this Scene (active row or saved row) is captured;
    /// anything else returns `null` like the JS.
    pub fn capture_scene_row_to_scene(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        if self.performance_enabled() {
            return Ok(Value::Null);
        }
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let Some(row) = params
            .get("rowIndex")
            .and_then(Value::as_i64)
            .filter(|row| *row >= 0)
            .map(|row| row as usize)
        else {
            return Ok(Value::Null);
        };
        let Some(index) =
            self.scenes.scenes.iter().position(|scene| {
                scene.get("id").and_then(Value::as_str) == Some(scene_id.as_str())
            })
        else {
            return Ok(Value::Null);
        };

        self.complete_whole_scene_transition();
        self.complete_row_transition(row);

        let Some(group) = self
            .state
            .groups
            .iter()
            .find(|group| {
                group.scene_id.as_deref() == Some(scene_id.as_str())
                    && group.scene_row == Some(row)
                    && group.transition_ghost != Some(true)
            })
            .cloned()
        else {
            // `scenes.js:3528-3530`: no matching Group means nothing to save.
            // A positional fallback here captured another Scene's live row.
            return Ok(Value::Null);
        };

        let group_value = group.for_preset_save();
        let mut modules = Map::new();
        for module_id in &group.modules {
            let Some(module) = self.state.modules.get(module_id) else {
                continue;
            };
            modules.insert(module_id.clone(), module.for_preset_save());
        }

        let row_names = self.row_names();
        let scene = self.scenes.scenes[index].clone();
        let scoped = collect_scoped_scene_data(&[Some(group_value.clone())], &modules, &self.state);
        let cell = json!({
            "group": group_value,
            "modules": Value::Object(modules),
            "inputs": scoped.get("inputs").cloned().unwrap_or(Value::Null),
            "expressions": scoped.get("expressions").cloned().unwrap_or(Value::Null),
            "tweens": scoped.get("tweens").cloned().unwrap_or(Value::Null),
        });

        let Some(data) = merge_scene_cell(&scene, row, &row_names, &cell) else {
            return Ok(Value::Null);
        };

        let mut updated = scene;
        if let (Value::Object(object), Value::Object(data)) = (&mut updated, &data) {
            for key in ["groups", "modules", "inputs", "expressions", "tweens"] {
                if let Some(value) = data.get(key) {
                    object.insert(key.to_string(), value.clone());
                }
            }
        }
        self.scenes.scenes[index] = updated.clone();
        self.touch_scene_list();
        self.deltas.push("scenes/ADD_SCENE", updated.clone());
        self.persist_best_effort();
        Ok(updated)
    }

    /// `scenes/captureLiveStateToScene` (`scenes.js:1956-2057`).
    ///
    /// Captures only the applied Scene's Groups (never ghosts, the Gallery or
    /// another Scene's Groups), rebuilds the Scene's `groups` **and** `modules`
    /// map from the live state, refuses while performance mode is running, and
    /// returns `{ error }` values the UI understands.
    pub fn capture_live_state_to_scene(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        let action = "scenes/captureLiveStateToScene";
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(action, "missing string \"sceneId\""))?
            .to_string();

        if self.performance_enabled() {
            return Ok(json!({ "error": "performance" }));
        }
        let Some(index) =
            self.scenes.scenes.iter().position(|scene| {
                scene.get("id").and_then(Value::as_str) == Some(scene_id.as_str())
            })
        else {
            return Ok(json!({ "error": "not-found" }));
        };
        let scene = self.scenes.scenes[index].clone();

        let live_values: Vec<Value> = self.state.groups.iter().map(json_value).collect();
        let row_names = normalize_row_names(self.scenes.performance.get("rowNames"));
        let row_count = get_row_count(&self.scenes.scenes, &live_values, &row_names);

        let (groups, from_live) = if scene_has_groups(&scene) {
            match capture_live_state(&self.state, &scene_id, row_count, false) {
                Ok(captured) => (captured.groups, false),
                Err(error) => {
                    self.warn_once(
                        format!("scene-capture:{scene_id}"),
                        &format!("engine: WARN {action}: {error}"),
                    );
                    return Ok(json!({ "error": "not-applied" }));
                }
            }
        } else {
            // An empty Scene column captures the current live layer stack
            // (`collectSceneGroups` `fromLive` case, `scene-capture.js:119-137`).
            match collect_scene_groups(&scene, &self.state.groups) {
                Some(collected) => (collected.groups, collected.from_live),
                None => return Ok(json!({ "error": "not-applied" })),
            }
        };

        let current_scene_id = self
            .scenes
            .performance
            .get("currentSceneId")
            .and_then(Value::as_str)
            .map(str::to_string);

        if from_live && current_scene_id.as_deref() == Some(scene_id.as_str()) {
            for (row, group) in groups.iter().enumerate() {
                let Some(group) = group.as_ref() else {
                    continue;
                };
                let Some(group_id) = group.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if let Some(live) = self.state.find_group_mut(group_id) {
                    live.scene_id = Some(scene_id.clone());
                    live.scene_row = Some(row);
                }
                self.deltas.push(
                    "groups/UPDATE_GROUP",
                    json!({
                        "groupId": group_id,
                        "data": { "sceneId": scene_id, "sceneRow": row },
                    }),
                );
            }
        }

        let mut modules = Map::new();
        for group in groups.iter().flatten() {
            let Some(module_ids) = group.get("modules").and_then(Value::as_array) else {
                continue;
            };
            for module_id in module_ids {
                let Some(module_id) = module_id.as_str() else {
                    continue;
                };
                let Some(module) = self.state.modules.get(module_id) else {
                    continue;
                };
                modules.insert(module_id.to_string(), module.for_preset_save());
            }
        }

        let scoped = collect_scoped_scene_data(&groups, &modules, &self.state);

        let mut updated = scene;
        if let Value::Object(object) = &mut updated {
            object.insert(
                "groups".to_string(),
                Value::Array(
                    groups
                        .into_iter()
                        .map(|group| group.unwrap_or(Value::Null))
                        .collect(),
                ),
            );
            object.insert("modules".to_string(), Value::Object(modules));
            if let Value::Object(scoped) = scoped {
                for (key, value) in scoped {
                    object.insert(key, value);
                }
            }
        }

        self.scenes.scenes[index] = updated.clone();
        self.touch_scene_list();
        self.deltas.push("scenes/ADD_SCENE", updated.clone());
        self.persist_best_effort();
        Ok(json!({ "scene": updated }))
    }

    /// `scenes/saveCurrentAsScene` (`scenes.js:1928-1954`).
    ///
    /// Snapshots the live preset-aware state (the same six-key document a
    /// preset export produces), adds the Scene metadata and appends it to the
    /// Scene list. `Scenes.vue` then writes the returned document to
    /// `scene/<name>.json`; before this action existed the save button hit the
    /// dispatch wildcard and silently did nothing.
    pub fn save_current_as_scene(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Scene {}", self.scenes.scenes.len() + 1));

        // `completeTransition` + `completeAllRowTransitions`: capture the
        // settled stack, never a half-faded ghost.
        self.complete_all_row_transitions();
        self.complete_whole_scene_transition();

        let timing = normalize_timing_config(Some(&self.scenes.performance));
        let mut scene = create_preset_from_state(&self.state);
        if let Value::Object(object) = &mut scene {
            object.insert("id".to_string(), json!(generate_scene_id()));
            object.insert("name".to_string(), json!(name));
            object.insert("enabled".to_string(), json!(true));
            object.insert("minSeconds".to_string(), json_number(timing.min));
            object.insert("maxSeconds".to_string(), json_number(timing.max));
            object.insert("timingOverride".to_string(), json!(false));
        }

        self.scenes.scenes.push(scene.clone());
        self.touch_scene_list();
        self.deltas.push("scenes/ADD_SCENE", scene.clone());
        self.persist_best_effort();
        Ok(scene)
    }

    /// `scenes/addEmptyScene` (`scenes.js:1892-1905`).
    pub fn add_empty_scene(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let timing = normalize_timing_config(Some(&self.scenes.performance));
        let timing = json!({
            "minSeconds": timing.min,
            "maxSeconds": timing.max,
        });
        let existing_names: Vec<Value> = self
            .scenes
            .scenes
            .iter()
            .map(|scene| scene.get("name").cloned().unwrap_or(Value::Null))
            .collect();
        let id = generate_scene_id();
        let scene = create_empty_scene(&EmptySceneOptions {
            id: &id,
            name: params.get("name").and_then(Value::as_str),
            existing_names: &existing_names,
            timing: Some(&timing),
        });

        self.scenes.scenes.push(scene.clone());
        self.touch_scene_list();
        self.deltas.push("scenes/ADD_SCENE", scene.clone());
        self.persist_best_effort();
        Ok(scene)
    }

    /// `scenes/moveScene` (`scenes.js:1911-1922`).
    ///
    /// Returns `true` when the list changed. The Scene order is the
    /// performance order, so this also changes which Scene runs next.
    pub fn move_scene(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let to_index = params
            .get("toIndex")
            .or_else(|| params.get("index"))
            .and_then(Value::as_i64);

        let Some(reordered) = move_scene_to_index(&self.scenes.scenes, scene_id, to_index) else {
            return Ok(json!(false));
        };

        self.scenes.scenes = reordered;
        self.touch_scene_list();
        self.emit_scene_list();
        self.persist_best_effort();
        Ok(json!(true))
    }

    /// `scenes/setSceneEnabled` (`scenes.js:3317-3330`).
    pub fn set_scene_enabled(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let enabled = match params.get("enabled") {
            Some(Value::Bool(flag)) => *flag,
            Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
            _ => false,
        };

        let Some(scene) = self.update_scene_document(scene_id, |scene| {
            scene.insert("enabled".to_string(), json!(enabled));
        }) else {
            return Ok(Value::Null);
        };
        Ok(scene)
    }

    /// `scenes/updateSceneName` (`scenes.js:2165-2187`).
    ///
    /// The name is trimmed and must not collide with another Scene
    /// (case-insensitive); a rejected rename returns `null` like the JS.
    pub fn update_scene_name(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        if name.is_empty() {
            return Ok(Value::Null);
        }

        let duplicate = self.scenes.scenes.iter().any(|scene| {
            scene.get("id").and_then(Value::as_str) != Some(scene_id)
                && scene
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|existing| existing.eq_ignore_ascii_case(&name))
        });
        if duplicate {
            return Ok(Value::Null);
        }

        let Some(scene) = self.update_scene_document(scene_id, |scene| {
            scene.insert("name".to_string(), json!(name));
        }) else {
            return Ok(Value::Null);
        };
        Ok(scene)
    }

    /// `scenes/updateSceneTiming` (`scenes.js:2188-2209`): writes an explicit
    /// per-Scene override, normalised against the performance timing.
    pub fn update_scene_timing(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(existing) = self
            .scenes
            .scenes
            .iter()
            .find(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id))
            .cloned()
        else {
            return Ok(Value::Null);
        };

        let current =
            normalize_scene_timing_values(Some(&existing), Some(&self.scenes.performance));
        let mut candidate = Map::new();
        candidate.insert(
            "minSeconds".to_string(),
            params
                .get("minSeconds")
                .cloned()
                .unwrap_or_else(|| json_number(current.min)),
        );
        candidate.insert(
            "maxSeconds".to_string(),
            params
                .get("maxSeconds")
                .cloned()
                .unwrap_or_else(|| json_number(current.max)),
        );
        candidate.insert("timingOverride".to_string(), json!(true));
        let timing = normalize_scene_timing_values(
            Some(&Value::Object(candidate)),
            Some(&self.scenes.performance),
        );

        let Some(scene) = self.update_scene_document(scene_id, |scene| {
            scene.insert("minSeconds".to_string(), json_number(timing.min));
            scene.insert("maxSeconds".to_string(), json_number(timing.max));
            scene.insert("timingOverride".to_string(), json!(true));
        }) else {
            return Ok(Value::Null);
        };
        Ok(scene)
    }

    /// `scenes/resetSceneTiming` (`scenes.js:2210-2231`): drops the override
    /// and re-inherits the global performance timing.
    pub fn reset_scene_timing(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let timing = normalize_timing_config(Some(&self.scenes.performance));

        let Some(scene) = self.update_scene_document(scene_id, |scene| {
            scene.insert("minSeconds".to_string(), json_number(timing.min));
            scene.insert("maxSeconds".to_string(), json_number(timing.max));
            scene.insert("timingOverride".to_string(), json!(false));
        }) else {
            return Ok(Value::Null);
        };
        Ok(scene)
    }

    /// `scenes/setTimingUnit` — switch the performance timing unit while
    /// keeping the configured durations.
    ///
    /// The global pair and every Scene with `timingOverride: true` are
    /// converted through seconds at the current BPM, then written back in
    /// `unit`. Scenes without an override are left untouched (their stored
    /// values are ignored until an override is created, at which point the
    /// global values are used). Emits the performance config and one
    /// `ADD_SCENE` per converted override, then persists.
    pub fn set_timing_unit(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let requested = TimingUnit::parse(params.get("unit").or_else(|| params.get("timingUnit")));
        let current = normalize_timing_config(Some(&self.scenes.performance));
        let bpm = self.bpm();

        let min_seconds = unit_to_seconds(current.min, current.unit, bpm);
        let max_seconds = unit_to_seconds(current.max, current.unit, bpm);
        let min = seconds_to_unit(min_seconds, requested, bpm);
        let max = seconds_to_unit(max_seconds, requested, bpm);

        if !self.scenes.performance.is_object() {
            self.scenes.performance = default_performance();
        }
        if let Some(object) = self.scenes.performance.as_object_mut() {
            object.insert(
                "timingUnit".to_string(),
                Value::String(requested.as_str().to_string()),
            );
            object.insert("minSeconds".to_string(), json_number(min));
            object.insert("maxSeconds".to_string(), json_number(max));
        }
        self.emit_performance();

        if requested != current.unit {
            let scene_ids: Vec<String> = self
                .scenes
                .scenes
                .iter()
                .filter(|scene| {
                    scene
                        .get("timingOverride")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .filter_map(|scene| scene.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect();

            for scene_id in scene_ids {
                self.update_scene_document(&scene_id, |scene| {
                    let scene_min = to_finite_number(scene.get("minSeconds"));
                    let scene_max = to_finite_number(scene.get("maxSeconds"));
                    let min_seconds =
                        unit_to_seconds(scene_min.unwrap_or(current.min), current.unit, bpm);
                    let max_seconds =
                        unit_to_seconds(scene_max.unwrap_or(current.max), current.unit, bpm);
                    scene.insert(
                        "minSeconds".to_string(),
                        json_number(seconds_to_unit(min_seconds, requested, bpm)),
                    );
                    scene.insert(
                        "maxSeconds".to_string(),
                        json_number(seconds_to_unit(max_seconds, requested, bpm)),
                    );
                });
            }
        }

        // `update_scene_document` already persisted each override; writing once
        // more keeps the performance delta and the document in sync when no
        // Scene changed. A no-op unit switch still emits the config so the
        // selector is authoritative.
        self.persist_best_effort();
        Ok(self.scenes.performance.clone())
    }

    /// Apply `mutate` to one Scene document, persist, and emit the upsert
    /// delta the mirror replays. Returns the updated Scene, or `None` when the
    /// id is unknown.
    fn update_scene_document(
        &mut self,
        scene_id: &str,
        mutate: impl FnOnce(&mut Map<String, Value>),
    ) -> Option<Value> {
        let index = self
            .scenes
            .scenes
            .iter()
            .position(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id))?;
        let scene = self.scenes.scenes[index].as_object_mut()?;
        mutate(scene);
        let updated = self.scenes.scenes[index].clone();
        self.touch_scene_list();
        self.deltas.push("scenes/ADD_SCENE", updated.clone());
        self.persist_best_effort();
        Some(updated)
    }

    /// `scenes/loadSceneFromJson` (`scenes.js:2059-2092`).
    ///
    /// Normalises a Scene document (including the legacy `group` + `layers`
    /// shape) and upserts it by id. `apply` defaults to `true`, matching the
    /// JS action: opening a Scene JSON applies it (the worker's `loadPreset`
    /// branch and File → Open Scene both rely on that). Importers that only
    /// want to add the document pass `"apply": false`.
    pub fn load_scene_from_json(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        let action = "scenes/loadSceneFromJson";
        let document = match params.get("scene").or_else(|| params.get("json")) {
            Some(Value::String(text)) => serde_json::from_str::<Value>(text)
                .map_err(|error| invalid(action, format!("invalid Scene JSON: {error}")))?,
            Some(value) => value.clone(),
            None => return Err(invalid(action, "missing \"scene\" or \"json\"")),
        };
        let Some(normalized) = modv_engine::scenes_playlist::normalize_scene_document(&document)
        else {
            return Ok(Value::Null);
        };
        let imported = with_scene_timing(&normalized, &self.scenes.performance);

        let scene_id = imported
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        match self
            .scenes
            .scenes
            .iter()
            .position(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id.as_str()))
        {
            Some(index) => self.scenes.scenes[index] = imported.clone(),
            None => self.scenes.scenes.push(imported.clone()),
        }
        self.touch_scene_list();
        self.deltas.push("scenes/ADD_SCENE", imported.clone());

        if params.get("apply").and_then(Value::as_bool) != Some(false) {
            let mut apply_params = Map::new();
            apply_params.insert("sceneId".to_string(), json!(scene_id));
            self.apply_scene_action(&apply_params)?;
        }
        if params.get("persist").and_then(Value::as_bool) != Some(false) {
            self.persist_best_effort();
        }
        Ok(imported)
    }

    /// `scenes/importPlaylist` (`scenes.js:2094-2163`).
    pub fn import_playlist_action(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        let action = "scenes/importPlaylist";
        let Some(playlist) = params.get("playlist") else {
            return Err(invalid(action, "missing \"playlist\""));
        };
        let mode = match params.get("mode").and_then(Value::as_str) {
            Some("replace") => ImportMode::Replace,
            Some("merge") | None => {
                if params.get("replace").and_then(Value::as_bool) == Some(true) {
                    ImportMode::Replace
                } else {
                    ImportMode::Merge
                }
            }
            Some(other) => return Err(invalid(action, format!("unknown mode \"{other}\""))),
        };

        let report = import_playlist_with(
            &mut self.scenes.scenes,
            &self.scenes.performance,
            playlist,
            mode,
        )
        .map_err(|error| invalid(action, error.to_string()))?;

        self.touch_scene_list();

        if let Some(performance) = &report.performance {
            self.scenes.performance = performance.clone();
            self.ensure_performance_runtime_keys();
            self.emit_performance();
        }

        // A playlist import changes the Scene list wholesale from the mirror's
        // point of view; `REPLACE_SCENES` is the mutation the JS
        // `importPlaylist` replace branch commits and it is exact for merge
        // too (the payload is the authoritative post-import list).
        self.emit_scene_list();

        let current = self
            .scenes
            .performance
            .get("currentSceneId")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(current) = current {
            let exists = self
                .scenes
                .scenes
                .iter()
                .any(|scene| scene.get("id").and_then(Value::as_str) == Some(current.as_str()));
            if !exists {
                self.set_performance_value("currentSceneId", Value::Null);
            }
        }

        self.persist_best_effort();
        Ok(json!({
            "scenes": self.scenes.scenes.clone(),
            "performance": self.scenes.performance.clone(),
            "added": report.added,
            "updated": report.updated,
            "skipped": report.skipped,
        }))
    }

    /// `scenes/exportPlaylist` (`scenes.js:2076-2088`).
    ///
    /// No file IO: the UI owns the save dialog (`export-scenes-playlist` in
    /// `src/main/window-prefs.js`).
    pub fn export_playlist_action(&self) -> Value {
        export_playlist(&self.scenes.scenes, &self.scenes.performance)
    }

    /// `scenes/removeScene` (`scenes.js:2817-2856`).
    pub fn remove_scene_action(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        let action = "scenes/removeScene";
        let scene_id = params
            .get("sceneId")
            .or_else(|| params.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(action, "missing string \"sceneId\""))?
            .to_string();

        // Finish in-flight transitions before deleting their Groups
        // (`scenes.js:2818-2819`): a row crossfade's ghost and a whole-scene
        // transition's outgoing Scene are stamped with this `sceneId`, so
        // removing them first would leave `row_transitions`/`transition`
        // pointing at dead ids that `scenes/tick` keeps writing to (R-13).
        self.complete_all_row_transitions();
        self.complete_whole_scene_transition();

        let group_ids: Vec<String> = self
            .state
            .groups
            .iter()
            .filter(|group| {
                group.name != GALLERY_GROUP_NAME
                    && group.scene_id.as_deref() == Some(scene_id.as_str())
            })
            .map(|group| group.id.clone())
            .collect();
        for group_id in group_ids {
            self.remove_live_group(&group_id);
        }

        if self.applied_scene_id.as_deref() == Some(scene_id.as_str()) {
            self.applied_scene_id = None;
        }
        if self
            .scenes
            .performance
            .get("currentSceneId")
            .and_then(Value::as_str)
            == Some(scene_id.as_str())
        {
            self.set_performance_value("currentSceneId", Value::Null);
        }

        let before = self.scenes.scenes.len();
        self.scenes
            .scenes
            .retain(|scene| scene.get("id").and_then(Value::as_str) != Some(scene_id.as_str()));
        let removed = before != self.scenes.scenes.len();
        self.touch_scene_list();

        self.deltas
            .push("scenes/REMOVE_SCENE", json!({ "sceneId": scene_id }));
        let active_rows = self.live_scene_rows();
        // `SET_ACTIVE_ROWS(state, activeRows)` takes the map directly
        // (`scenes.js:4156-4158`).
        self.deltas.push("scenes/SET_ACTIVE_ROWS", active_rows);

        self.persist_best_effort();
        Ok(json!({ "removed": removed }))
    }

    /// `scenes/setRowNames` (`scenes.js:3376-3397`).
    /// Naming/reordering a row also remaps the live Groups so the grid and the
    /// layer stack snap into place (`remapRowGroupsForNames`, `scenes.js:1247-1315`).
    pub fn set_row_names(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let normalized = normalize_row_names(params.get("rowNames"));
        self.set_performance_value(
            "rowNames",
            Value::Array(normalized.iter().cloned().map(Value::String).collect()),
        );
        self.remap_row_groups_for_names(&normalized);
        self.touch_scene_list();
        self.emit_group_order();
        self.persist_best_effort();
        Ok(Value::Array(
            normalized.into_iter().map(Value::String).collect(),
        ))
    }

    /// `scenes/setCellEnabled` (native-only): toggle one Scene cell's
    /// performance-mode flag.
    ///
    /// The flag lives in `performance.disabledCells` (`{ "<sceneId>": [row] }`,
    /// sorted and de-duplicated). The JS Scenes grid ignores the key and keeps
    /// saving an otherwise-identical `scenes.json`; manual `applyScene`/cell
    /// clicks are unaffected, only the performance picks skip the cell.
    pub fn set_cell_enabled(&mut self, params: &Map<String, Value>) -> Result<Value, StoreError> {
        let action = "scenes/setCellEnabled";
        let scene_id = required_str(params, action, "sceneId")?.to_string();
        let row = params
            .get("rowIndex")
            .and_then(Value::as_u64)
            .map(|row| row as usize)
            .ok_or_else(|| invalid(action, "missing numeric \"rowIndex\""))?;
        let enabled = params
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let row_names = self.row_names();
        let Some(scene) = self
            .scenes
            .scenes
            .iter()
            .find(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id.as_str()))
        else {
            return Ok(json!({ "updated": false, "reason": "scene-not-found" }));
        };
        if get_row_group_index(scene, row, &row_names).is_none() {
            return Ok(json!({ "updated": false, "reason": "empty-cell" }));
        }

        let mut disabled = normalize_disabled_cells_for_scenes(
            self.scenes.performance.get("disabledCells"),
            &self.scenes.scenes,
            &row_names,
        );
        set_scene_cell_disabled(&mut disabled, &scene_id, row, !enabled);
        self.set_performance_value("disabledCells", disabled_cells_json(&disabled));
        self.touch_scene_list();
        self.persist_best_effort();
        Ok(json!({
            "sceneId": scene_id,
            "rowIndex": row,
            "enabled": enabled,
        }))
    }

    /// `scenes/updatePerformance` (`scenes.js:3332-3340`).
    ///
    /// Normalises the incoming timing/transition data and merges it over the
    /// current config. `enabled` follows the merged config (the JS tick reads
    /// `state.performance.enabled`), while `currentSceneId` survives unless
    /// the payload provides one.
    pub fn update_performance_action(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        let data = params
            .get("config")
            .cloned()
            .unwrap_or_else(|| Value::Object(params.clone()));
        let mut merged = merge_performance(&self.scenes.performance, &data);
        if merged.get("disabledCells").is_some() {
            let row_names = normalize_row_names(merged.get("rowNames"));
            let disabled = normalize_disabled_cells_for_scenes(
                merged.get("disabledCells"),
                &self.scenes.scenes,
                &row_names,
            );
            if let Some(object) = merged.as_object_mut() {
                object.insert("disabledCells".to_string(), disabled_cells_json(&disabled));
            }
        }
        // A config update can carry `rowNames` (the grid mapping a preview
        // resolves); a changed mapping invalidates cached Scene previews.
        let row_names_changed = merged.get("rowNames") != self.scenes.performance.get("rowNames");
        self.scenes.performance = merged;
        self.performance.enabled = self.performance_enabled();
        if row_names_changed {
            self.touch_scene_list();
        }
        self.ensure_performance_runtime_keys();
        self.emit_performance();
        self.persist_best_effort();
        Ok(self.scenes.performance.clone())
    }

    /// `scenes/setPerformanceEnabled` (`scenes.js:3152-3178`).
    ///
    /// M5b stores the flag and config only. The dwell/crossfade runtime loop
    /// (`scenes/tick`, `startPerformance`, `stopPerformance`) is M5c; leaving
    /// this a pure state mutation keeps the UI toggle and `scenes.json`
    /// consistent until then.
    pub fn set_performance_enabled(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Value, StoreError> {
        let enabled = params
            .get("enabled")
            .map(|value| {
                value
                    .as_bool()
                    .unwrap_or_else(|| value.as_f64().map(|number| number != 0.0).unwrap_or(false))
            })
            .unwrap_or(false);
        self.performance.enabled = enabled;
        if !enabled {
            self.performance.rows.clear();
        }
        self.set_performance_value("enabled", Value::Bool(enabled));
        self.persist_best_effort();
        Ok(Value::Bool(enabled))
    }

    // ————————————————— internals —————————————————

    /// `true` when performance mode is running (`state.performance.enabled`).
    pub(crate) fn performance_enabled(&self) -> bool {
        self.scenes
            .performance
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Emit the Scene list as `REPLACE_SCENES` (`scenes.js:4106-4108`).
    ///
    /// The JS hydration/seeding path commits one `ADD_SCENE` per Scene; a
    /// single authoritative `REPLACE_SCENES` produces the identical mirror
    /// state and one record instead of dozens.
    fn emit_scene_list(&mut self) {
        let scenes = Value::Array(self.scenes.scenes.clone());
        self.deltas.push("scenes/REPLACE_SCENES", scenes);
    }

    /// Emit the performance config (`UPDATE_PERFORMANCE`, `scenes.js:4148-4150`).
    fn emit_performance(&mut self) {
        self.deltas
            .push("scenes/UPDATE_PERFORMANCE", self.scenes.performance.clone());
    }

    /// Emit the seeding metadata mutations (`scenes.js:4178-4188`).
    fn emit_defaults_metadata(&mut self) {
        self.deltas.push(
            "scenes/SET_SEEDED_DEFAULT_SCENES",
            Value::Array(
                self.scenes
                    .seeded_default_scenes
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        self.deltas.push(
            "scenes/SET_DEFAULTS_SEEDED",
            json!(self.scenes.defaults_seeded),
        );
        self.deltas.push(
            "scenes/SET_DEFAULTS_VERSION",
            json!(self.scenes.defaults_version),
        );
    }

    /// `groups/orderByIds` (`groups.js:248-254`) as its `REPLACE_GROUPS`
    /// mutation: non-Gallery Groups sorted by `sceneRow`, Gallery last.
    fn emit_group_order(&mut self) {
        let mut ordered: Vec<Group> = self
            .state
            .groups
            .iter()
            .filter(|group| group.name != GALLERY_GROUP_NAME)
            .cloned()
            .collect();
        ordered.sort_by(|a, b| {
            let row_a = a.scene_row.unwrap_or(usize::MAX);
            let row_b = b.scene_row.unwrap_or(usize::MAX);
            // Within a row the outgoing (ghost) half draws first, so the
            // incoming Group renders on top (`orderRowGroups` in
            // `scenes.js:1300-1316` sorts `transitionGhost` before everything
            // else; the later entry wins in the compositor's draw order).
            let ghost_a = a.transition_ghost == Some(true);
            let ghost_b = b.transition_ghost == Some(true);
            row_a.cmp(&row_b).then_with(|| ghost_b.cmp(&ghost_a))
        });

        let gallery: Vec<Group> = self
            .state
            .groups
            .iter()
            .filter(|group| group.name == GALLERY_GROUP_NAME)
            .cloned()
            .collect();
        let mut final_groups = ordered;
        final_groups.extend(gallery);
        self.state.groups = final_groups;

        let value = Value::Array(self.state.groups.iter().map(json_value).collect());
        self.deltas
            .push("groups/REPLACE_GROUPS", json!({ "groups": value }));
    }

    /// `groups/removeGroup` + `cleanUpGroupResources` (`groups.js:204-244`).
    pub(crate) fn remove_live_group(&mut self, group_id: &str) {
        let Some(index) = self
            .state
            .groups
            .iter()
            .position(|group| group.id == group_id)
        else {
            return;
        };
        // A link editor opened on this Group/one of its Modules may still name
        // an Input the release is about to remove; keep the control identities
        // so its Save can be retargeted.
        let retired_inputs = self.capture_input_identities();
        let group = self.state.groups.remove(index);
        self.release_group(group);
        self.retire_replaced_inputs(retired_inputs);
    }

    /// Release a Group already detached from `state.groups`: drop Modules no
    /// other Group still references, remove its control inputs and emit the
    /// mirror's `REMOVE_GROUP`.
    fn release_group(&mut self, group: Group) {
        for module_id in &group.modules {
            let still_used = self
                .state
                .groups
                .iter()
                .any(|other| other.modules.iter().any(|id| id == module_id));
            if still_used {
                continue;
            }
            self.drop_active_module(module_id);
        }

        for (_, key) in GROUP_CONTROL_INPUTS {
            if let Some(input_id) = group_input_id(&group, key) {
                self.remove_input(&input_id);
            }
        }

        self.deltas
            .push("groups/REMOVE_GROUP", json!({ "id": group.id }));
    }

    fn drop_active_module(&mut self, module_id: &str) {
        let Some(module) = self.state.modules.shift_remove(module_id) else {
            return;
        };
        for key in MODULE_META_INPUTS {
            if let Some(input_id) = module
                .meta
                .get(format!("{key}InputId"))
                .and_then(Value::as_str)
            {
                self.remove_input(input_id);
            }
        }
        if let Some(props) = module.dollar_props.as_object() {
            for descriptor in props.values() {
                let Some(input_id) = descriptor.get("id").and_then(Value::as_str) else {
                    continue;
                };
                self.remove_input(input_id);
                // "clear up datatypes with multiple inputs" (`modules.js:798-819`):
                // each component sub-input `${id}-${key}` is removed with its
                // parent.
                let type_name = descriptor
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                for key in modv_engine::inputs::data_type_input_keys(type_name) {
                    self.remove_input(&format!("{input_id}-{key}"));
                }
            }
        }
        self.deltas.push(
            "modules/REMOVE_ACTIVE_MODULE",
            json!({ "moduleId": module_id }),
        );
    }

    fn remove_input(&mut self, input_id: &str) {
        self.state.input_links.shift_remove(input_id);
        self.state.inputs.shift_remove(input_id);
        self.deltas
            .push("inputs/REMOVE_INPUT_LINK", json!({ "inputId": input_id }));
        self.deltas
            .push("inputs/REMOVE_INPUT", json!({ "inputId": input_id }));
    }

    /// Build one live Group from a Scene Group snapshot.
    ///
    /// Mirrors the `createGroup` argument filtering in `applySceneNow`
    /// (`scenes.js:2556-2601`): ids/inputs/`modules` are replaced, everything
    /// else (including unknown keys) is kept.
    fn build_live_group(
        &mut self,
        snapshot: &Map<String, Value>,
        scene_id: &str,
        row: usize,
        input_map: &mut HashMap<String, String>,
    ) -> Group {
        let id = crate::ids::uuid_v4();
        let mut group = Group {
            id: id.clone(),
            name: snapshot
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("New Group")
                .to_string(),
            hidden: snapshot
                .get("hidden")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            modules: Vec::new(),
            enabled: int_value(snapshot.get("enabled")).unwrap_or(GROUP_ENABLED),
            alpha: snapshot.get("alpha").and_then(Value::as_f64).unwrap_or(1.0),
            inherit: snapshot
                .get("inherit")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            inherit_from: inherit_from(snapshot.get("inheritFrom")),
            pipeline: bool_value(snapshot.get("pipeline")),
            clearing: bool_value(snapshot.get("clearing")),
            composite_operation: snapshot
                .get("compositeOperation")
                .and_then(Value::as_str)
                .unwrap_or("normal")
                .to_string(),
            scene_id: Some(scene_id.to_string()),
            scene_row: Some(row),
            transition_ghost: None,
            draw_to_canvas_id: None,
            alpha_input_id: None,
            enabled_input_id: None,
            clearing_input_id: None,
            inherit_input_id: None,
            composite_operation_input_id: None,
            pipeline_input_id: None,
            extra: Map::new(),
        };

        for (key, value) in snapshot {
            if !GROUP_KNOWN_KEYS.contains(&key.as_str()) {
                group.extra.insert(key.clone(), value.clone());
            }
        }

        for (control, key) in GROUP_CONTROL_INPUTS {
            let input_id = crate::ids::uuid_v4();
            let input = Input {
                id: input_id.clone(),
                type_: "commit".to_string(),
                data: json!({ "groupId": id, "key": control }),
                get_location: None,
                location: Some("groups/UPDATE_GROUP_BY_KEY".to_string()),
            };
            self.insert_input(input);
            set_group_input_id(&mut group, key, Some(input_id.clone()));
            if let Some(old_id) = snapshot.get(*key).and_then(Value::as_str) {
                input_map.insert(old_id.to_string(), input_id);
            }
        }

        group
    }

    /// Build one live Module from a Scene Module snapshot.
    ///
    /// Prop values are applied **on top of** the registry defaults and the
    /// snapshot's `meta` is merged on top of the registry meta. Unknown module
    /// names still become active records with the `$status` error entry
    /// (`modules.js:253-268`), so an old Scene never crashes an apply.
    fn build_live_module(
        &mut self,
        snapshot: &Value,
        input_map: &mut HashMap<String, String>,
    ) -> Option<String> {
        let object = snapshot.as_object()?;
        let module_name = object
            .get("$moduleName")
            .and_then(Value::as_str)
            .or_else(|| {
                object
                    .get("meta")
                    .and_then(|meta| meta.get("name"))
                    .and_then(Value::as_str)
            })
            .or_else(|| object.get("name").and_then(Value::as_str))?
            .to_string();
        let id = crate::ids::uuid_v4();

        let (mut meta, defaults, registered) = match self.registry.meta_fields(&module_name) {
            Some(meta) => (
                meta,
                self.registry
                    .default_props(&module_name)
                    .unwrap_or_else(|| json!({})),
                true,
            ),
            None => (Map::new(), json!({}), false),
        };

        // JS defaults then the snapshot's meta (`modules.js:344-359`).
        meta.insert("name".to_string(), json!(module_name));
        meta.insert("alpha".to_string(), json!(1));
        meta.insert("enabled".to_string(), json!(false));
        meta.insert("compositeOperation".to_string(), json!("normal"));
        if let Some(snapshot_meta) = object.get("meta").and_then(Value::as_object) {
            for (key, value) in snapshot_meta {
                meta.insert(key.clone(), value.clone());
            }
        }
        // Older Scene documents store `enabled: 1` / `0`; the legacy renderer
        // tests it for truthiness while the engine compositor requires a real
        // boolean, so a restored Webcam/effect would otherwise be skipped.
        modv_engine::engine_state::normalise_module_meta(&mut meta);
        meta.entry("isGallery".to_string())
            .or_insert_with(|| json!(false));

        let mut props = defaults.as_object().cloned().unwrap_or_default();
        if let Some(snapshot_props) = object.get("props").and_then(Value::as_object) {
            for (prop, value) in snapshot_props {
                let coerced = self
                    .registry
                    .prop_schema(&module_name, prop)
                    .map(|schema| coerce_prop_value(&schema, value))
                    .unwrap_or_else(|| value.clone());
                props.insert(prop.clone(), coerced);
            }
        }

        let old_descriptors = object
            .get("$props")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let mut dollar_props = Map::new();
        for prop in props.keys() {
            let schema = self.registry.prop_schema(&module_name, prop);
            let old_descriptor = old_descriptors.get(prop);
            let mut descriptor = old_descriptor
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let input_id = crate::ids::uuid_v4();
            let input = Input {
                id: input_id.clone(),
                type_: "action".to_string(),
                data: json!({ "moduleId": id, "prop": prop }),
                get_location: Some(format!("modules.active[\"{id}\"].props[\"{prop}\"]")),
                location: Some("modules/updateProp".to_string()),
            };
            self.insert_input(input);
            descriptor.insert("id".to_string(), json!(input_id));

            // `initialiseModuleProperties` (`modules.js:91-113`) with
            // `generateNewIds`: the data-type component sub-inputs are minted
            // for the live module too, so a Scene-applied color/vec prop keeps
            // its per-component link rows.
            if let Some(schema) = &schema {
                for key in modv_engine::inputs::data_type_input_keys(&schema.type_) {
                    let input = Input {
                        id: format!("{input_id}-{key}"),
                        type_: "action".to_string(),
                        data: json!({
                            "moduleId": id,
                            "prop": prop,
                            "path": format!("[{key}]"),
                        }),
                        get_location: Some(format!(
                            "modules.active[\"{id}\"].props[\"{prop}\"][\"{key}\"]"
                        )),
                        location: Some("modules/updateProp".to_string()),
                    };
                    self.insert_input(input);
                }
            }

            if let Some(schema) = &schema {
                descriptor
                    .entry("type".to_string())
                    .or_insert_with(|| json!(schema.type_));
                if let Some(min) = schema.min {
                    descriptor.insert("min".to_string(), json_number(min));
                }
                if let Some(max) = schema.max {
                    descriptor.insert("max".to_string(), json_number(max));
                }
            }
            dollar_props.insert(prop.clone(), Value::Object(descriptor.clone()));
            if let Some(old_id) = old_descriptor
                .and_then(|descriptor| descriptor.get("id"))
                .and_then(Value::as_str)
            {
                input_map.insert(old_id.to_string(), input_id.clone());
                // `mapModuleInputIds` (`scenes.js:426-433`) remaps each
                // data-type sub-input `${old id}-${key}` → `${new id}-${key}`,
                // using the union of the old and new prop types.
                let old_type = old_descriptor
                    .and_then(|descriptor| descriptor.get("type"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let new_type = schema
                    .as_ref()
                    .map(|schema| schema.type_.as_str())
                    .unwrap_or_default();
                let mut keys: Vec<&str> =
                    modv_engine::inputs::data_type_input_keys(old_type).to_vec();
                for key in modv_engine::inputs::data_type_input_keys(new_type) {
                    if !keys.contains(key) {
                        keys.push(key);
                    }
                }
                for key in keys {
                    input_map.insert(format!("{old_id}-{key}"), format!("{input_id}-{key}"));
                }
            }
        }

        for key in MODULE_META_INPUTS {
            let input_id = crate::ids::uuid_v4();
            let input = Input {
                id: input_id.clone(),
                type_: "action".to_string(),
                data: json!({ "id": id, "metaKey": key }),
                get_location: Some(format!("modules.active[\"{id}\"].meta.{key}")),
                location: Some("modules/updateMeta".to_string()),
            };
            self.insert_input(input);
            meta.insert(format!("{key}InputId"), json!(input_id));
            if let Some(old_id) = object
                .get("meta")
                .and_then(|meta| meta.get(format!("{key}InputId")))
                .and_then(Value::as_str)
            {
                input_map.insert(old_id.to_string(), input_id);
            }
        }

        let status = if registered {
            Vec::new()
        } else {
            vec![json!({
                "type": "error",
                "message": format!(
                    "Module \"{module_name}\" is not registered. VmodV will skip this while rendering"
                ),
            })]
        };

        let module = ModuleInstance {
            id: id.clone(),
            module_name,
            meta: Value::Object(meta),
            props: Value::Object(props),
            data: object.get("data").cloned().unwrap_or_else(|| json!({})),
            dollar_props: Value::Object(dollar_props),
            status,
            extra: Map::new(),
        };

        let value = module_json(&module);
        self.state.modules.insert(id.clone(), module);
        self.deltas
            .push("modules/ADD_ACTIVE_MODULE", json!({ "module": value }));
        Some(id)
    }

    fn insert_input(&mut self, input: Input) {
        let value = json_value(&input);
        self.state.inputs.insert(input.id.clone(), input);
        self.deltas
            .push("inputs/ADD_INPUT", json!({ "input": value }));
    }

    /// Insert a Group at its row position without disturbing the Gallery group
    /// or Groups that belong to no Scene.
    fn insert_group_at_row(&mut self, group: Group) {
        let row = group.scene_row.unwrap_or(usize::MAX);
        let user_positions: Vec<usize> = self
            .state
            .groups
            .iter()
            .enumerate()
            .filter(|(_, existing)| existing.name != GALLERY_GROUP_NAME)
            .map(|(index, _)| index)
            .collect();
        let index = match user_positions.get(row) {
            Some(position) => *position,
            None => user_positions
                .last()
                .map(|position| position + 1)
                .unwrap_or(self.state.groups.len()),
        };
        self.state.groups.insert(index, group);
    }

    /// Remap string `inheritFrom` targets to the freshly created Group ids
    /// (`scenes.js:2679-2703`).
    fn remap_scene_inherit(&mut self, group_snapshots: &[Value], id_map: &HashMap<String, String>) {
        for snapshot in group_snapshots.iter().filter_map(Value::as_object) {
            let Some(old_target) = snapshot.get("inheritFrom").and_then(Value::as_str) else {
                continue;
            };
            let Some(old_id) = snapshot.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(new_id) = id_map.get(old_id) else {
                continue;
            };
            let inherit = id_map
                .get(old_target)
                .cloned()
                .map(InheritFrom::Group)
                .unwrap_or(InheritFrom::Previous(-1));
            if let Some(group) = self.state.find_group_mut(new_id) {
                group.inherit_from = inherit.clone();
            }
            let value = match &inherit {
                InheritFrom::Group(id) => json!(id),
                InheritFrom::Previous(index) => json!(index),
            };
            self.deltas.push(
                "groups/UPDATE_GROUP",
                json!({ "groupId": new_id, "data": { "inheritFrom": value } }),
            );
        }
    }

    /// `restoreSceneInputs` (`scenes.js:432-539`) without the focused-input
    /// tail (see [`Self::restore_scene_focused_input`], which only a
    /// whole-Scene apply runs).
    fn restore_scene_inputs(&mut self, scene: &Value, input_map: &HashMap<String, String>) {
        let links = scene
            .get("inputs")
            .and_then(|inputs| inputs.get("inputLinks"))
            .or_else(|| scene.get("inputLinks"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let tweens = scene_tween_entries(scene);

        // Tweens first: a tween link's location points at the new tween id
        // (`scenes.js:466-506`).
        let mut created_tweens: HashSet<String> = HashSet::new();
        for (key, tween) in &tweens {
            let tween_id = tween
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(key)
                .to_string();
            let Some(new_id) = input_map.get(&tween_id) else {
                continue;
            };
            if !created_tweens.insert(new_id.clone()) {
                continue;
            }
            let mut value = tween.clone();
            if let Value::Object(object) = &mut value {
                object.insert("id".to_string(), json!(new_id));
            }
            push_tween(&mut self.state.tweens, value.clone());
            self.deltas.push("tweens/ADD_TWEEN", value.clone());
            self.register_tween(&value);
        }

        for (old_id, link) in links {
            let Some(new_id) = input_map.get(&old_id) else {
                continue;
            };
            let mut value = link.clone();
            let is_tween = value.get("source").and_then(Value::as_str) == Some("tween");
            let Some(object) = value.as_object_mut() else {
                continue;
            };
            object.insert("id".to_string(), json!(new_id));
            if is_tween {
                object.insert(
                    "location".to_string(),
                    json!(format!("tweens.tweens['{new_id}'].value[0]")),
                );
            }
            let Ok(input_link) = serde_json::from_value::<InputLink>(value.clone()) else {
                continue;
            };
            self.state.input_links.insert(new_id.clone(), input_link);
            self.deltas
                .push("inputs/ADD_INPUT_LINK", json!({ "inputLink": value }));
        }

        // No binding rebuild here: `register_tween` already upserted each of
        // the document's tweens, and a reload would reset the playhead of every
        // surviving tween to frame 0 on each Scene advance — including tweens
        // on rows that stay (`scenes.js` never rebuilds them, R-8). Only ids
        // that left the authoritative list are dropped.
        self.prune_tweens();
    }

    /// The focused-input tail of `restoreSceneInputs` (`scenes.js:503-537`).
    ///
    /// The saved id is remapped through the same old→new input map the links
    /// and tweens use; when the Scene no longer creates that input the focus
    /// is cleared (`inputs/clearFocusedInput`). A document without an `inputs`
    /// object (the legacy `layers` shape) leaves the focus alone, and a
    /// row/scoped apply never calls this — the JS `scoped` early-return.
    fn restore_scene_focused_input(&mut self, scene: &Value, input_map: &HashMap<String, String>) {
        let Some(inputs) = scene.get("inputs").filter(|inputs| inputs.is_object()) else {
            return;
        };
        let focused = inputs.get("focusedInput");
        let title = focused
            .and_then(|focused| focused.get("title"))
            .cloned()
            .unwrap_or(Value::Null);
        let new_id = focused
            .and_then(|focused| focused.get("id"))
            .and_then(Value::as_str)
            .and_then(|old_id| input_map.get(old_id));

        match new_id {
            Some(new_id) => self.set_focused_input_values(json!(new_id), title),
            None => self.clear_focused_input_values(),
        }
    }

    /// `restoreSceneMidi` (`scenes.js:593-599`): a Scene's `midi` block is a
    /// full MIDI state loaded through `midi/loadPresetData` (`SET_STATE`).
    /// A missing or non-object block is ignored, exactly like the JS guard.
    fn restore_scene_midi(&mut self, scene: &Value) {
        let Some(midi) = scene.get("midi").filter(|midi| midi.is_object()) else {
            return;
        };
        if let Err(error) = self.midi_set_state(midi) {
            self.warn_once(
                "scene-apply-midi".to_string(),
                &format!("engine: WARN scenes/applyScene: could not restore MIDI: {error}"),
            );
        }
    }

    /// Restore a Scene document's expression assignments (`scenes.js`
    /// `restoreSceneExpressions`, `scenes.js:534-592`).
    ///
    /// `input_map` maps the document's input ids to the freshly created live
    /// ids. `stale_input_ids` selects between the two JS modes:
    ///
    /// * `None` — whole-Scene apply: replace every live assignment with the
    ///   document's mapped assignments (`loadPresetData` + `SWAP`).
    /// * `Some(ids)` — row apply: remove the assignments whose input the
    ///   replaced Group/Modules owned, then merge the mapped ones.
    ///
    /// Assignments whose input is not in the map (the Scene's row did not
    /// create it) and the identity expression `"value"` are skipped, exactly
    /// like `restoreSceneExpressions` skipping a missing `newInputId` and
    /// `expressions/create` returning `null` for `"value"`.
    fn restore_scene_expressions(
        &mut self,
        scene: &Value,
        input_map: &HashMap<String, String>,
        stale_input_ids: Option<&HashSet<String>>,
    ) {
        let source = scene
            .get("expressions")
            .and_then(|expressions| expressions.get("assignments"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        let mut assignments: Vec<modv_engine::preset::ExpressionAssignment> = Vec::new();
        for (key, raw) in &source {
            let Some(object) = raw.as_object() else {
                continue;
            };
            let Some(old_input_id) = object.get("inputId").and_then(Value::as_str) else {
                continue;
            };
            let Some(new_input_id) = input_map.get(old_input_id) else {
                continue;
            };
            let expression = object
                .get("expression")
                .and_then(Value::as_str)
                .unwrap_or("value");
            if expression.trim() == "value" {
                continue;
            }

            let mut value = raw.clone();
            let Some(map) = value.as_object_mut() else {
                continue;
            };
            map.insert("inputId".to_string(), json!(new_input_id));
            map.entry("id".to_string()).or_insert_with(|| json!(key));
            let Ok(assignment) =
                serde_json::from_value::<modv_engine::preset::ExpressionAssignment>(value)
            else {
                continue;
            };
            assignments.push(assignment);
        }

        let removed: Vec<String> = self
            .state
            .expressions
            .iter()
            .filter(|(_, assignment)| match stale_input_ids {
                Some(stale) => stale.contains(&assignment.input_id),
                None => true,
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in removed {
            self.state.expressions.shift_remove(&id);
            self.deltas
                .push("expressions/REMOVE_EXPRESSION", json!({ "id": id }));
        }

        for assignment in assignments {
            self.state
                .expressions
                .insert(assignment.id.clone(), assignment.clone());
            let value = serde_json::to_value(&assignment).unwrap_or(Value::Null);
            self.deltas.push(
                "expressions/ADD_EXPRESSION",
                json!({ "assignment": value, "writeToSwap": false }),
            );
        }

        self.rebuild_expressions();
    }

    /// Active rows implied by the live Group stamps for one Scene
    /// (`scenes.js:2709-2726`).
    fn scene_active_rows(&self, scene_id: &str) -> Value {
        let mut rows = Map::new();
        for group in &self.state.groups {
            if group.scene_id.as_deref() != Some(scene_id) || group.transition_ghost == Some(true) {
                continue;
            }
            let Some(row) = group.scene_row else {
                continue;
            };
            rows.insert(
                row.to_string(),
                json!({ "sceneId": scene_id, "groupId": group.id }),
            );
        }
        Value::Object(rows)
    }

    /// Active rows implied by every live Scene-stamped Group, as
    /// `row -> (scene id, group id)`.
    ///
    /// This is the typed source behind `scenes/SET_ACTIVE_ROWS` and the native
    /// snapshot's per-row live markers. A transition ghost is the outgoing
    /// half of a row crossfade, so it must not claim the row; a Group with no
    /// Scene stamp or no row is not a Scene cell.
    pub(crate) fn live_scene_rows_by_row(&self) -> BTreeMap<usize, (String, String)> {
        let mut rows = BTreeMap::new();
        for group in &self.state.groups {
            let Some(scene_id) = &group.scene_id else {
                continue;
            };
            let Some(row) = group.scene_row else {
                continue;
            };
            if group.transition_ghost == Some(true) {
                continue;
            }
            rows.insert(row, (scene_id.clone(), group.id.clone()));
        }
        rows
    }

    /// Active rows implied by every live Scene-stamped Group.
    pub(crate) fn live_scene_rows(&self) -> Value {
        let mut rows = Map::new();
        for (row, (scene_id, group_id)) in self.live_scene_rows_by_row() {
            rows.insert(
                row.to_string(),
                json!({ "sceneId": scene_id, "groupId": group_id }),
            );
        }
        Value::Object(rows)
    }

    fn set_current_scene(&mut self, scene_id: &str) {
        self.set_performance_value("currentSceneId", json!(scene_id));
    }

    pub(crate) fn set_performance_value(&mut self, key: &str, value: Value) {
        if !self.scenes.performance.is_object() {
            self.scenes.performance = default_performance();
        }
        if let Some(object) = self.scenes.performance.as_object_mut() {
            object.insert(key.to_string(), value.clone());
        }
        self.deltas
            .push("scenes/UPDATE_PERFORMANCE", json!({ key: value }));
    }

    /// `normalizePerformanceTiming` over the config, keeping the non-timing
    /// keys (`enabled`, `order`, `rowNames`, `allowedTransitions`).
    fn ensure_performance_runtime_keys(&mut self) {
        if !self.scenes.performance.is_object() {
            self.scenes.performance = default_performance();
        }
        if let Some(object) = self.scenes.performance.as_object_mut() {
            object
                .entry("enabled".to_string())
                .or_insert_with(|| json!(false));
            object
                .entry("currentSceneId".to_string())
                .or_insert(Value::Null);
        }
    }

    /// `remapRowGroupsForNames` (`scenes.js:1247-1315`).
    fn remap_row_groups_for_names(&mut self, names: &[String]) {
        let live: Vec<Value> = self
            .state
            .groups
            .iter()
            .filter(|group| {
                group.name != GALLERY_GROUP_NAME && group.transition_ghost != Some(true)
            })
            .map(json_value)
            .collect();
        let ids: Vec<String> = self
            .state
            .groups
            .iter()
            .filter(|group| {
                group.name != GALLERY_GROUP_NAME && group.transition_ghost != Some(true)
            })
            .map(|group| group.id.clone())
            .collect();

        let mut taken_groups: HashSet<usize> = HashSet::new();
        let mut taken_rows: HashSet<usize> = HashSet::new();
        let mut updates: Vec<(String, usize)> = Vec::new();

        for (row_index, name) in names.iter().enumerate() {
            if name.trim().is_empty() {
                continue;
            }
            let mut found = live.iter().enumerate().find_map(|(index, group)| {
                (!taken_groups.contains(&index) && strict_matches_row_name(group, name))
                    .then_some(index)
            });
            if found.is_none() && !row_family_is_shared(row_index, names) {
                found = live.iter().enumerate().find_map(|(index, group)| {
                    (!taken_groups.contains(&index) && matches_row_name(group, name))
                        .then_some(index)
                });
            }
            if let Some(index) = found {
                taken_groups.insert(index);
                taken_rows.insert(row_index);
                updates.push((ids[index].clone(), row_index));
            }
        }

        let mut remaining: Vec<(usize, usize)> = live
            .iter()
            .enumerate()
            .filter(|(index, _)| !taken_groups.contains(index))
            .map(|(index, group)| {
                let row = group
                    .get("sceneRow")
                    .and_then(Value::as_u64)
                    .map(|row| row as usize)
                    .unwrap_or(index);
                (index, row)
            })
            .collect();
        remaining.sort_by_key(|(index, row)| (*row, *index));

        for (index, row) in remaining {
            let mut target = row.max(names.len());
            while taken_rows.contains(&target) {
                target += 1;
            }
            taken_rows.insert(target);
            updates.push((ids[index].clone(), target));
        }

        for (group_id, row) in updates {
            let current = self
                .state
                .find_group(&group_id)
                .and_then(|group| group.scene_row);
            if current == Some(row) {
                continue;
            }
            if let Some(group) = self.state.find_group_mut(&group_id) {
                group.scene_row = Some(row);
            }
            self.deltas.push(
                "groups/UPDATE_GROUP",
                json!({ "groupId": group_id, "data": { "sceneRow": row } }),
            );
        }
    }

    pub(crate) fn persist_best_effort(&self) {
        if let Err(error) = self.persist_scenes() {
            eprintln!("engine: WARN could not save the Scene list: {error}");
        }
    }
}

/// One Group created by [`Store::apply_scene_action`] before it is inserted.
struct CreatedGroup {
    row: usize,
    old_group_id: Option<String>,
    group: Group,
}

/// `buildSceneSnapshot` (`scenes.js:1689-1835`): materialise a default Scene
/// definition into a full Scene document without touching the live stack.
///
/// The returned [`BuiltDefaultScene`] also carries the smoothing ids the
/// document's explicit bass links minted, so the store can seed them.
fn build_default_scene_document(
    registry: &ModuleRegistry,
    definition: &SceneDocument,
    midi: &Value,
    skipped_modules: &mut usize,
) -> BuiltDefaultScene {
    let mut groups: Vec<Value> = Vec::new();
    let mut modules: Map<String, Value> = Map::new();
    let mut inputs: Map<String, Value> = Map::new();
    let mut input_links: Map<String, Value> = Map::new();
    let mut tweens: Vec<Value> = Vec::new();

    for group_definition in &definition.groups {
        let group_id = crate::ids::uuid_v4();
        let mut group = Map::new();
        group.insert("id".to_string(), json!(group_id));
        group.insert("name".to_string(), json!(group_definition.name));
        group.insert("hidden".to_string(), json!(false));
        group.insert("modules".to_string(), json!([]));
        group.insert("enabled".to_string(), json!(GROUP_ENABLED));
        group.insert(
            "alpha".to_string(),
            json_number(group_definition.alpha.unwrap_or(1.0)),
        );
        group.insert(
            "inherit".to_string(),
            json!(group_definition.inherit.unwrap_or(true)),
        );
        group.insert("inheritFrom".to_string(), json!(-1));
        group.insert(
            "pipeline".to_string(),
            json!(bool_value(group_definition.extra.get("pipeline"))),
        );
        // `buildSceneSnapshot` clears by default (`scenes.js:1710`).
        group.insert(
            "clearing".to_string(),
            json!(group_definition.clearing.unwrap_or(true)),
        );
        group.insert(
            "compositeOperation".to_string(),
            json!(
                group_definition
                    .composite_operation
                    .clone()
                    .unwrap_or_else(|| "normal".to_string())
            ),
        );
        for (control, key) in GROUP_CONTROL_INPUTS {
            let input_id = crate::ids::uuid_v4();
            inputs.insert(
                input_id.clone(),
                json!({
                    "id": input_id,
                    "type": "commit",
                    "data": { "groupId": group_id, "key": control },
                    "location": "groups/UPDATE_GROUP_BY_KEY",
                }),
            );
            group.insert((*key).to_string(), json!(input_id));
        }
        for (key, value) in &group_definition.extra {
            if key != "pipeline" {
                group.insert(key.clone(), value.clone());
            }
        }

        let mut group_module_ids: Vec<Value> = Vec::new();
        for module_definition in &group_definition.modules {
            let Some(meta_fields) = registry.meta_fields(&module_definition.name) else {
                // `buildSceneSnapshot` skips unregistered Modules
                // (`scenes.js:1733-1740`).
                *skipped_modules += 1;
                continue;
            };
            let module_id = crate::ids::uuid_v4();

            let mut meta = meta_fields;
            meta.insert("alpha".to_string(), json!(1));
            // Snapshots never render, but the snapshot's `enabled` default is
            // `true` (`scenes.js:1748`).
            meta.insert("enabled".to_string(), json!(true));
            meta.insert("compositeOperation".to_string(), json!("normal"));
            if let Some(overrides) = &module_definition.meta {
                for (key, value) in overrides {
                    meta.insert(key.clone(), value.clone());
                }
            }
            meta.insert("isGallery".to_string(), json!(false));

            let mut props = registry
                .default_props(&module_definition.name)
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default();
            if let Some(overrides) = &module_definition.props {
                for (prop, value) in overrides {
                    let coerced = registry
                        .prop_schema(&module_definition.name, prop)
                        .map(|schema| coerce_prop_value(&schema, value))
                        .unwrap_or_else(|| value.clone());
                    props.insert(prop.clone(), coerced);
                }
            }

            let mut dollar_props = Map::new();
            for prop in props.keys() {
                let input_id = crate::ids::uuid_v4();
                inputs.insert(
                    input_id.clone(),
                    json!({
                        "id": input_id,
                        "type": "action",
                        "data": { "moduleId": module_id, "prop": prop },
                        "getLocation": format!(
                            "modules.active[\"{module_id}\"].props[\"{prop}\"]"
                        ),
                        "location": "modules/updateProp",
                    }),
                );
                let mut descriptor = Map::new();
                descriptor.insert("id".to_string(), json!(input_id));
                if let Some(schema) = registry.prop_schema(&module_definition.name, prop) {
                    descriptor.insert("type".to_string(), json!(schema.type_));
                    if let Some(min) = schema.min {
                        descriptor.insert("min".to_string(), json_number(min));
                    }
                    if let Some(max) = schema.max {
                        descriptor.insert("max".to_string(), json_number(max));
                    }
                }
                dollar_props.insert(prop.clone(), Value::Object(descriptor));
            }

            for key in MODULE_META_INPUTS {
                let input_id = crate::ids::uuid_v4();
                inputs.insert(
                    input_id.clone(),
                    json!({
                        "id": input_id,
                        "type": "action",
                        "data": { "id": module_id, "metaKey": key },
                        "getLocation": format!("modules.active[\"{module_id}\"].meta.{key}"),
                        "location": "modules/updateMeta",
                    }),
                );
                meta.insert(format!("{key}InputId"), json!(input_id));
            }

            let module = json!({
                "$id": module_id,
                "$moduleName": module_definition.name,
                "meta": Value::Object(meta),
                "props": Value::Object(props),
                "data": {},
                "$props": Value::Object(dollar_props),
            });
            group_module_ids.push(json!(module_id));
            modules.insert(module_id, module);
        }
        group.insert("modules".to_string(), Value::Array(group_module_ids));
        groups.push(Value::Object(group));
    }

    // `applyDefaultSceneInputLinks` (`scenes.js:930-1024`): pinned links first,
    // then one automatic bass link and one automatic BPM tween.
    let mut smoothing_ids: Vec<String> = Vec::new();
    let mut build = SceneBuild {
        groups: &groups,
        modules: &modules,
        input_links: &mut input_links,
        tweens: &mut tweens,
        smoothing_ids: &mut smoothing_ids,
    };
    apply_default_scene_input_links(definition, &mut build);

    let mut document = Map::new();
    document.insert("id".to_string(), json!(generate_scene_id()));
    document.insert("name".to_string(), json!(definition.name));
    document.insert("enabled".to_string(), json!(true));
    if let Some(min) = definition.min_seconds {
        document.insert("minSeconds".to_string(), json_number(min));
    }
    if let Some(max) = definition.max_seconds {
        document.insert("maxSeconds".to_string(), json_number(max));
    }
    document.insert("timingOverride".to_string(), json!(false));
    document.insert("groups".to_string(), Value::Array(groups));
    document.insert("modules".to_string(), Value::Object(modules));
    document.insert(
        "inputs".to_string(),
        json!({
            "focusedInput": { "id": Value::Null, "title": Value::Null },
            "inputs": Value::Object(inputs),
            "inputLinks": Value::Object(input_links),
        }),
    );
    document.insert("expressions".to_string(), json!({ "assignments": {} }));
    document.insert("tweens".to_string(), Value::Array(tweens));
    document.insert("midi".to_string(), midi.clone());
    BuiltDefaultScene {
        document: Value::Object(document),
        smoothing_ids,
    }
}

/// A default Scene materialised from its definition, plus the smoothing ids
/// its explicit bass links minted.
struct BuiltDefaultScene {
    /// The Scene document (`buildSceneSnapshot`).
    document: Value,
    /// `meyda/getSmoothingId` ids the document's links reference; the store
    /// seeds them in its smoothing registry (`addSmoothingId` starts at `0`).
    smoothing_ids: Vec<String>,
}

/// The mutable pieces of a Scene snapshot under construction.
struct SceneBuild<'a> {
    groups: &'a [Value],
    modules: &'a Map<String, Value>,
    input_links: &'a mut Map<String, Value>,
    tweens: &'a mut Vec<Value>,
    /// Smoothing ids minted for explicit bass links.
    smoothing_ids: &'a mut Vec<String>,
}

/// `applyDefaultSceneInputLinks` (`scenes.js:930-1024`).
fn apply_default_scene_input_links(definition: &SceneDocument, build: &mut SceneBuild<'_>) {
    let mut excluded: HashSet<String> = HashSet::new();

    // Pinned links from the definition first (`applyExplicitSceneInputLinks`,
    // `scenes.js:889-928`).
    for link in &definition.input_links {
        let Some(target) = resolve_scene_link_target(build, link) else {
            eprintln!(
                "engine: WARN default Scene \"{}\" could not link \"{}\" control \"{}\"",
                definition.name, link.group, link.control
            );
            continue;
        };
        let range = modv_engine::scene_link_defaults::LinkRange {
            min: link.min.unwrap_or(target.fallback_range.min),
            max: link.max.unwrap_or(target.fallback_range.max),
        };
        if link.source.as_deref() == Some("tween") {
            create_bpm_tween_link(
                build,
                &target,
                range,
                link.bpm_division.map(|value| json!(value)),
            );
        } else {
            create_bass_link(
                build,
                &target,
                link.feature.as_deref().unwrap_or("bass"),
                range,
                link.smoothing,
            );
        }
        excluded.insert(target.input_id);
    }

    // Automatic bass link (`scenes.js:961-989`).
    if let Some(target) = find_module_link_target(
        build,
        LINK_GROUP_PRIORITY,
        BASS_PROP_PREFERENCE,
        &excluded,
        LinkMode::Bass,
    ) {
        let range = target
            .module
            .as_ref()
            .map(|module| get_bass_link_range(module, &target.prop_name))
            .unwrap_or(modv_engine::scene_link_defaults::LinkRange {
                min: 0.55,
                max: 1.0,
            });
        create_bass_link(build, &target, "bass", range, None);
        excluded.insert(target.input_id);
    } else if let Some(target) = find_group_alpha_target(build, LINK_GROUP_PRIORITY, &excluded) {
        let range = modv_engine::scene_link_defaults::LinkRange {
            min: 0.55,
            max: 1.0,
        };
        create_bass_link(build, &target, "bass", range, None);
        excluded.insert(target.input_id);
    }

    // Automatic BPM tween (`scenes.js:991-1017`).
    if let Some(target) = find_module_link_target(
        build,
        LINK_GROUP_PRIORITY,
        BPM_PROP_PREFERENCE,
        &excluded,
        LinkMode::Bpm,
    ) {
        let range = target
            .module
            .as_ref()
            .map(|module| get_bpm_link_range(module, &target.prop_name))
            .unwrap_or(modv_engine::scene_link_defaults::LinkRange { min: 0.4, max: 1.0 });
        create_bpm_tween_link(build, &target, range, None);
    } else if let Some(target) = find_group_alpha_target(build, BPM_GROUP_PRIORITY, &excluded) {
        let range = modv_engine::scene_link_defaults::LinkRange { min: 0.4, max: 1.0 };
        create_bpm_tween_link(build, &target, range, None);
    }
}

/// A resolved link target inside the Scene snapshot under construction.
struct LinkTarget {
    input_id: String,
    module: Option<Value>,
    prop_name: String,
    fallback_range: modv_engine::scene_link_defaults::LinkRange,
}

fn resolve_scene_link_target(
    build: &SceneBuild<'_>,
    link: &modv_engine::default_scenes::SceneInputLink,
) -> Option<LinkTarget> {
    let group_index = build
        .groups
        .iter()
        .position(|group| group.get("name").and_then(Value::as_str) == Some(link.group.as_str()))?;
    let group = &build.groups[group_index];
    let control = if link.control.is_empty() {
        "alpha"
    } else {
        link.control.as_str()
    };

    let Some(module_name) = link.module.as_deref() else {
        let input_id = group_control_input_id(group, control)?;
        return Some(LinkTarget {
            input_id,
            module: None,
            prop_name: control.to_string(),
            fallback_range: modv_engine::scene_link_defaults::LinkRange { min: 0.2, max: 1.0 },
        });
    };

    let module = find_module_in_group(build, group, module_name)?;
    let input_id = module_control_input_id(&module, control)?;
    Some(LinkTarget {
        input_id,
        module: Some(module),
        prop_name: control.to_string(),
        fallback_range: modv_engine::scene_link_defaults::LinkRange { min: 0.2, max: 1.0 },
    })
}

fn find_module_in_group(build: &SceneBuild<'_>, group: &Value, module_name: &str) -> Option<Value> {
    let ids = group.get("modules").and_then(Value::as_array)?;
    ids.iter().find_map(|id| {
        let id = id.as_str()?;
        let module = build.modules.get(id)?;
        (module.get("$moduleName").and_then(Value::as_str) == Some(module_name))
            .then(|| module.clone())
    })
}

fn module_control_input_id(module: &Value, control: &str) -> Option<String> {
    if MODULE_META_INPUTS.contains(&control) {
        return module
            .get("meta")
            .and_then(|meta| meta.get(format!("{control}InputId")))
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    module
        .get("$props")
        .and_then(|props| props.get(control))
        .and_then(|descriptor| descriptor.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn group_control_input_id(group: &Value, control: &str) -> Option<String> {
    let key = GROUP_CONTROL_INPUTS
        .iter()
        .find(|(name, _)| *name == control)
        .map(|(_, key)| *key)?;
    group.get(key).and_then(Value::as_str).map(str::to_string)
}

/// `findModuleLinkTarget` (`scenes.js:723-764`): search the priority Groups
/// first, then every other Group, in draw order.
fn find_module_link_target(
    build: &SceneBuild<'_>,
    priority: &[&str],
    preference: &[&str],
    excluded: &HashSet<String>,
    mode: LinkMode,
) -> Option<LinkTarget> {
    let mut order: Vec<usize> = Vec::new();
    for name in priority {
        if let Some(index) = build
            .groups
            .iter()
            .position(|group| group.get("name").and_then(Value::as_str) == Some(*name))
            && !order.contains(&index)
        {
            order.push(index);
        }
    }
    for index in 0..build.groups.len() {
        if !order.contains(&index) {
            order.push(index);
        }
    }

    for group_index in order {
        let group = &build.groups[group_index];
        let Some(module_ids) = group.get("modules").and_then(Value::as_array) else {
            continue;
        };
        for module_id in module_ids {
            let Some(module) = module_id
                .as_str()
                .and_then(|id| build.modules.get(id))
                .cloned()
            else {
                continue;
            };
            let Some(prop_name) = pick_link_prop(&module, preference, excluded, mode) else {
                continue;
            };
            let Some(input_id) = module_control_input_id(&module, &prop_name) else {
                continue;
            };
            return Some(LinkTarget {
                input_id,
                module: Some(module),
                prop_name,
                fallback_range: modv_engine::scene_link_defaults::LinkRange { min: 0.4, max: 1.0 },
            });
        }
    }
    None
}

/// `pickLinkProp` (`scenes.js:647-690`): numeric, unused and not excluded by
/// the automatic-link exclusion table.
fn pick_link_prop(
    module: &Value,
    preference: &[&str],
    excluded: &HashSet<String>,
    mode: LinkMode,
) -> Option<String> {
    let props = module.get("$props").and_then(Value::as_object)?;
    let module_name = module.get("$moduleName").and_then(Value::as_str);
    let available = |name: &str| -> bool {
        let Some(descriptor) = props.get(name) else {
            return false;
        };
        let numeric = matches!(
            descriptor.get("type").and_then(Value::as_str),
            Some("int") | Some("float")
        );
        let input_id = descriptor
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        numeric
            && !excluded.contains(input_id)
            && !is_auto_link_excluded(module_name, name, mode.as_str())
    };

    for name in preference {
        if available(name) {
            return Some((*name).to_string());
        }
    }
    props
        .keys()
        .find(|name| available(name))
        .map(|name| name.to_string())
}

/// The Group-alpha fallback for a Scene with no numeric Module control
/// (`scenes.js:966-989`).
fn find_group_alpha_target(
    build: &SceneBuild<'_>,
    priority: &[&str],
    excluded: &HashSet<String>,
) -> Option<LinkTarget> {
    let mut candidates: Vec<usize> = Vec::new();
    for name in priority {
        if let Some(index) = build
            .groups
            .iter()
            .position(|group| group.get("name").and_then(Value::as_str) == Some(*name))
            && !candidates.contains(&index)
        {
            candidates.push(index);
        }
    }
    for index in 0..build.groups.len() {
        if !candidates.contains(&index) {
            candidates.push(index);
        }
    }

    for group_index in candidates {
        let group = &build.groups[group_index];
        if let Some(input_id) = group_control_input_id(group, "alpha")
            && !excluded.contains(&input_id)
        {
            return Some(LinkTarget {
                input_id,
                module: None,
                prop_name: "alpha".to_string(),
                fallback_range: modv_engine::scene_link_defaults::LinkRange {
                    min: 0.55,
                    max: 1.0,
                },
            });
        }
    }
    None
}

fn create_bass_link(
    build: &mut SceneBuild<'_>,
    target: &LinkTarget,
    feature: &str,
    range: modv_engine::scene_link_defaults::LinkRange,
    smoothing: Option<f64>,
) {
    let link = default_link_for_input(LinkMode::Bass, &target.input_id, Some(feature), Some(range));
    let feature = link.feature.clone().unwrap_or_else(|| "bass".to_string());

    // `createBassLink` (`scenes.js:770-778`): a usable smoothing amount mints a
    // `meyda/getSmoothingId` id and widens the args to
    // `[feature, smoothingId, smoothingValue]`; without one the getter reads
    // the raw feature.
    let mut args = vec![json!(feature)];
    if let Some(amount) = smoothing.and_then(|amount| get_feature_smoothing(Some(&json!(amount)))) {
        let smoothing_id = crate::ids::uuid_v4();
        build.smoothing_ids.push(smoothing_id.clone());
        args.push(json!(smoothing_id));
        args.push(json_number(amount));
    }

    let value = json!({
        "id": target.input_id,
        "type": link.r#type,
        "location": link.location,
        "source": link.source,
        "args": args,
        "min": json_number(link.range.min),
        "max": json_number(link.range.max),
    });
    build.input_links.insert(target.input_id.clone(), value);
}

fn create_bpm_tween_link(
    build: &mut SceneBuild<'_>,
    target: &LinkTarget,
    range: modv_engine::scene_link_defaults::LinkRange,
    bpm_division: Option<Value>,
) {
    let link = default_link_for_input(LinkMode::Bpm, &target.input_id, None, Some(range));
    build
        .tweens
        .push(default_link_tween(&target.input_id, bpm_division.as_ref()));
    let value = json!({
        "id": target.input_id,
        "type": link.r#type,
        "location": link.location,
        "source": link.source,
        "min": json_number(link.range.min),
        "max": json_number(link.range.max),
    });
    build.input_links.insert(target.input_id.clone(), value);
}

/// `createScopedSceneData` (`scenes.js:331-373`): the input/link/expression/
/// tween slice of the live state that belongs to a Scene's Groups and Modules.
fn collect_scoped_scene_data(
    groups: &[Option<Value>],
    modules: &Map<String, Value>,
    state: &modv_engine::engine_state::EngineState,
) -> Value {
    let mut input_ids: HashSet<String> = HashSet::new();
    for group in groups.iter().flatten() {
        for (_, key) in GROUP_CONTROL_INPUTS {
            if let Some(input_id) = group.get(*key).and_then(Value::as_str) {
                input_ids.insert(input_id.to_string());
            }
        }
    }
    for module in modules.values() {
        for key in MODULE_META_INPUTS {
            if let Some(input_id) = module
                .get("meta")
                .and_then(|meta| meta.get(format!("{key}InputId")))
                .and_then(Value::as_str)
            {
                input_ids.insert(input_id.to_string());
            }
        }
        if let Some(props) = module.get("$props").and_then(Value::as_object) {
            for descriptor in props.values() {
                if let Some(input_id) = descriptor.get("id").and_then(Value::as_str) {
                    input_ids.insert(input_id.to_string());
                }
            }
        }
    }

    let mut inputs = Map::new();
    let mut input_links = Map::new();
    let mut tweens: Vec<Value> = Vec::new();

    for input_id in &input_ids {
        if let Some(input) = state.inputs.get(input_id) {
            inputs.insert(input_id.clone(), json_value(input));
        }
        let Some(link) = state.input_links.get(input_id) else {
            continue;
        };
        input_links.insert(input_id.clone(), json_value(link));
        if link.source.as_deref() == Some("tween")
            && let Some(tween) = find_tween(&state.tweens, input_id)
        {
            tweens.push(tween);
        }
    }

    let mut assignments = Map::new();
    for (id, assignment) in &state.expressions {
        if input_ids.contains(&assignment.input_id) {
            assignments.insert(id.clone(), json_value(assignment));
        }
    }

    json!({
        "inputs": {
            "focusedInput": { "id": Value::Null, "title": Value::Null },
            "inputs": Value::Object(inputs),
            "inputLinks": Value::Object(input_links),
        },
        "expressions": { "assignments": Value::Object(assignments) },
        "tweens": Value::Array(tweens),
        "midi": state.midi.clone(),
    })
}

fn find_tween(tweens: &Value, id: &str) -> Option<Value> {
    match tweens {
        Value::Array(entries) => entries
            .iter()
            .find(|tween| tween.get("id").and_then(Value::as_str) == Some(id))
            .cloned(),
        Value::Object(entries) => entries.get(id).cloned(),
        _ => None,
    }
}

fn push_tween(tweens: &mut Value, tween: Value) {
    if !tweens.is_array() {
        *tweens = Value::Array(Vec::new());
    }
    if let Value::Array(entries) = tweens {
        let id = tween.get("id").and_then(Value::as_str).map(str::to_string);
        if let Some(id) = id {
            entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(id.as_str()));
        }
        entries.push(tween);
    }
}

fn scene_tween_entries(scene: &Value) -> Vec<(String, Value)> {
    match scene.get("tweens") {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|tween| {
                let id = tween.get("id").and_then(Value::as_str)?.to_string();
                Some((id, tween.clone()))
            })
            .collect(),
        Some(Value::Object(entries)) => entries
            .iter()
            .map(|(key, tween)| (key.clone(), tween.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

/// `writeStoredScenes` (`scenes.js:1660-1683`) minus IO: normalise a Scene
/// document's timing against the current performance config.
fn with_scene_timing(scene: &Value, performance: &Value) -> Value {
    let timing = normalize_scene_timing_values(Some(scene), Some(performance));
    let mut document = scene.as_object().cloned().unwrap_or_default();
    document.insert("minSeconds".to_string(), json_number(timing.min));
    document.insert("maxSeconds".to_string(), json_number(timing.max));
    document.insert("timingOverride".to_string(), json!(timing.timing_override));
    Value::Object(document)
}

/// Parse a `scenes.json` document (`hydrateScenes`, `scenes.js:2835-2899`).
fn parse_stored_scenes(stored: &Value) -> Option<ParsedStoredScenes> {
    let object = stored.as_object()?;

    let stored_performance = object
        .get("performance")
        .or_else(|| object.get("autopilot"))
        .filter(|value| value.is_object());

    let mut performance = default_performance();
    if let Some(stored) = stored_performance {
        performance = merge_performance(&performance, stored);
    }
    // `hydrateScenes` `UPDATE_PERFORMANCE`s the stored config over the
    // defaults, so a legacy `autopilot.enabled` resumes performance mode
    // exactly like the Electron spec. The stored `currentSceneId` is runtime
    // state: [`Store::hydrate_scenes`] keeps the live value instead of the
    // stored one, mirroring `currentSceneId: state.performance.currentSceneId`.

    let mut scenes = Vec::new();
    if let Some(entries) = object.get("scenes").and_then(Value::as_array) {
        for entry in entries {
            if let Some(normalized) = modv_engine::scenes_playlist::normalize_scene_document(entry)
            {
                scenes.push(with_scene_timing(&normalized, &performance));
            }
        }
    }

    let stored_version = object
        .get("defaultsVersion")
        .and_then(|value| to_finite_number(Some(value)))
        .filter(|version| *version >= 0.0)
        .map(|version| version.floor() as i64);
    let stored_seeded = object
        .get("defaultsSeeded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut seeded = normalize_default_scene_names(object.get("seededDefaultScenes"));
    let mut defaults_version = stored_version.unwrap_or({
        if stored_seeded {
            LEGACY_DEFAULTS_VERSION
        } else {
            0
        }
    });
    if seeded.is_empty() && defaults_version > 0 {
        seeded = get_default_scene_names_up_to(defaults_version);
    }
    if stored_version.is_none() && !stored_seeded && seeded.is_empty() {
        defaults_version = 0;
    }

    // Native-only per-cell flags: prune unknown Scenes/rows so a stale
    // `scenes.json` cannot disable a cell that no longer exists. The JS UI
    // ignores `disabledCells` and keeps it in the document.
    let row_names = normalize_row_names(performance.get("rowNames"));
    if performance.get("disabledCells").is_some() {
        let disabled = normalize_disabled_cells_for_scenes(
            performance.get("disabledCells"),
            &scenes,
            &row_names,
        );
        if let Some(object) = performance.as_object_mut() {
            object.insert("disabledCells".to_string(), disabled_cells_json(&disabled));
        }
    }

    Some(ParsedStoredScenes {
        scenes,
        performance,
        defaults_version,
        defaults_seeded: stored_seeded || defaults_version > 0 || !seeded.is_empty(),
        seeded_default_scenes: seeded,
    })
}

/// Merge performance data over a config, normalising the timing keys
/// (`scenes.js:2106-2113`).
fn merge_performance(current: &Value, data: &Value) -> Value {
    let mut merged = current.as_object().cloned().unwrap_or_default();
    if let Some(normalized) = normalize_performance_data(data).as_object() {
        for (key, value) in normalized {
            merged.insert(key.clone(), value.clone());
        }
    }
    let timing = normalize_timing_config(Some(&Value::Object(merged.clone())));
    merged.insert(
        "timingUnit".to_string(),
        Value::String(timing.unit.as_str().to_string()),
    );
    merged.insert("minSeconds".to_string(), json_number(timing.min));
    merged.insert("maxSeconds".to_string(), json_number(timing.max));
    merged.insert(
        "transitionSeconds".to_string(),
        json_number(timing.transition_seconds),
    );
    if merged.contains_key("allowedTransitions") {
        let list = normalize_transition_list(merged.get("allowedTransitions"));
        merged.insert(
            "allowedTransitions".to_string(),
            Value::Array(list.into_iter().map(Value::String).collect()),
        );
    }
    Value::Object(merged)
}

/// `normalizeDefaultSceneNames` (`scenes.js:104-130`).
fn normalize_default_scene_names(value: Option<&Value>) -> Vec<String> {
    let Some(entries) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for entry in entries {
        let Some(name) = entry.as_str() else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let key = scene_key(name);
        if seen.insert(key) {
            names.push(name.to_string());
        }
    }
    names
}

/// De-duplicate a stored list of default Scene names by the same case-insensitive
/// key (`normalizeDefaultSceneNames` over an already-string list).
fn normalize_default_scene_name_list(names: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if seen.insert(scene_key(name)) {
            result.push(name.to_string());
        }
    }
    result
}

/// Read and parse a JSON file; `Ok(None)` when it does not exist.
fn read_json_file(path: &Path) -> io::Result<Option<Value>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(Some(value)),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not valid JSON: {error}", path.display()),
        )),
    }
}

fn required_str<'a>(
    params: &'a Map<String, Value>,
    action: &str,
    key: &str,
) -> Result<&'a str, StoreError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(action, format!("missing string \"{key}\"")))
}

fn invalid(action: &str, detail: impl Into<String>) -> StoreError {
    StoreError::InvalidPayload {
        action: action.to_string(),
        detail: detail.into(),
    }
}

/// Serialise an integral `f64` the way JS writes it.
fn json_number(value: f64) -> Value {
    const SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    if value.fract() == 0.0 && value.abs() <= SAFE_INTEGER {
        json!(value as i64)
    } else {
        json!(value)
    }
}

fn int_value(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number as i64))
        .or_else(|| value.as_bool().map(i64::from))
}

/// JS truthiness for the values a Group flag can take.
fn bool_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty() && text != "false" && text != "0",
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Null) | None => false,
    }
}

fn inherit_from(value: Option<&Value>) -> InheritFrom {
    match value {
        Some(Value::String(id)) => InheritFrom::Group(id.clone()),
        Some(other) => other
            .as_i64()
            .or_else(|| other.as_f64().map(|number| number as i64))
            .map(InheritFrom::Previous)
            .unwrap_or_default(),
        None => InheritFrom::default(),
    }
}

fn set_group_input_id(group: &mut Group, key: &str, value: Option<String>) {
    match key {
        "alphaInputId" => group.alpha_input_id = value,
        "enabledInputId" => group.enabled_input_id = value,
        "clearingInputId" => group.clearing_input_id = value,
        "inheritInputId" => group.inherit_input_id = value,
        "compositeOperationInputId" => group.composite_operation_input_id = value,
        "pipelineInputId" => group.pipeline_input_id = value,
        _ => {}
    }
}

fn group_input_id(group: &Group, key: &str) -> Option<String> {
    match key {
        "alphaInputId" => group.alpha_input_id.clone(),
        "enabledInputId" => group.enabled_input_id.clone(),
        "clearingInputId" => group.clearing_input_id.clone(),
        "inheritInputId" => group.inherit_input_id.clone(),
        "compositeOperationInputId" => group.composite_operation_input_id.clone(),
        "pipelineInputId" => group.pipeline_input_id.clone(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! Pure-state tests for the Scene store. No GPU, no window: every case
    //! drives [`Store::dispatch`] and inspects the typed state and the delta
    //! stream the mirror replays.
    use super::*;
    use crate::store_ops::Store;
    use modv_engine::engine_state::GALLERY_GROUP_NAME;
    use modv_renderers::two_d::AudioFrame;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "modv-scenes-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn delta_types(store: &mut Store) -> Vec<String> {
        store
            .drain_deltas()
            .into_iter()
            .map(|mutation| mutation.r#type)
            .collect()
    }

    /// A two-row Scene document: Background with a `Bands` module and Effects
    /// with a `Ball` module.
    fn two_row_scene(id: &str) -> Value {
        json!({
            "id": id,
            "name": "Test Scene",
            "groups": [
                {
                    "id": "doc-bg",
                    "name": "Background",
                    "hidden": false,
                    "modules": ["doc-bands"],
                    "enabled": 1,
                    "alpha": 1,
                    "inherit": false,
                    "inheritFrom": -1,
                    "pipeline": false,
                    "clearing": true,
                    "compositeOperation": "normal",
                },
                {
                    "id": "doc-effects",
                    "name": "Effects",
                    "hidden": false,
                    "modules": ["doc-ball"],
                    "enabled": 1,
                    "alpha": 0.5,
                    "inherit": true,
                    "inheritFrom": "doc-bg",
                    "pipeline": false,
                    "clearing": true,
                    "compositeOperation": "screen",
                },
            ],
            "modules": {
                "doc-bands": {
                    "$id": "doc-bands",
                    "$moduleName": "Bands",
                    "meta": { "name": "Bands", "enabled": true },
                    "props": { "u_ringWidth": 0.3 },
                    "data": {},
                    "$props": {},
                },
                "doc-ball": {
                    "$id": "doc-ball",
                    "$moduleName": "Ball",
                    "meta": { "name": "Ball", "enabled": true },
                    "props": { "amount": 3 },
                    "data": {},
                    "$props": {},
                },
            },
        })
    }

    fn find_default_scene<'a>(store: &'a Store, name: &str) -> &'a Value {
        store
            .scenes_state()
            .scenes
            .iter()
            .find(|scene| scene["name"] == json!(name))
            .unwrap_or_else(|| panic!("{name} still exists"))
    }

    fn seed_scene(store: &mut Store, scene: Value) {
        store
            .dispatch(
                "scenes/loadSceneFromJson",
                json!({ "scene": scene, "apply": false, "persist": false }),
            )
            .expect("scene loads");
    }

    /// The bind-side input id a default Scene link targets, resolved from the
    /// built document (Group → Module → control input).
    fn scene_link_input_id(
        scene: &Value,
        link: &modv_engine::default_scenes::SceneInputLink,
    ) -> String {
        let group = scene["groups"]
            .as_array()
            .expect("groups")
            .iter()
            .find(|group| group["name"] == json!(link.group))
            .unwrap_or_else(|| panic!("group {}", link.group));
        let Some(module_name) = link.module.as_deref() else {
            return group_control_input_id(group, &link.control).expect("group control input");
        };
        let module_id = group["modules"]
            .as_array()
            .expect("group modules")
            .iter()
            .filter_map(Value::as_str)
            .find(|id| scene["modules"][*id]["$moduleName"] == json!(module_name))
            .unwrap_or_else(|| panic!("module {module_name} in group {}", link.group));
        module_control_input_id(&scene["modules"][module_id], &link.control)
            .expect("module control input")
    }

    // ————————————————— applyScene —————————————————

    #[test]
    fn apply_scene_mints_and_remaps_component_sub_inputs() {
        // A Scene snapshot's color prop carries its saved sub-input ids in
        // `inputs.inputLinks` keyed `${old parent}-r`; applying the Scene
        // mints fresh sub-inputs for the live module and remaps the link
        // (`mapModuleInputIds`, `scenes.js:426-433`).
        let scene = json!({
            "id": "scene-components",
            "name": "Component scene",
            "groups": [{
                "id": "doc-text-group",
                "name": "Background",
                "hidden": false,
                "modules": ["doc-text-module"],
                "enabled": 1,
                "alpha": 1,
                "inherit": false,
                "inheritFrom": -1,
                "pipeline": false,
                "clearing": true,
                "compositeOperation": "normal",
            }],
            "modules": {
                "doc-text-module": {
                    "$id": "doc-text-module",
                    "$moduleName": "Text",
                    "meta": { "name": "Text", "enabled": true },
                    "props": { "fillColor": {"r": 0.2, "g": 0.4, "b": 0.6, "a": 1.0} },
                    "data": {},
                    "$props": { "fillColor": { "id": "doc-fill", "type": "color" } },
                },
            },
            "inputs": {
                "focusedInput": { "id": null, "title": null },
                "inputs": {
                    "doc-fill-r": {
                        "id": "doc-fill-r",
                        "type": "action",
                        "getLocation": "modules.active[\"doc-text-module\"].props[\"fillColor\"][\"r\"]",
                        "location": "modules/updateProp",
                        "data": { "moduleId": "doc-text-module", "prop": "fillColor", "path": "[r]" }
                    }
                },
                "inputLinks": {
                    "doc-fill-r": {
                        "id": "doc-fill-r",
                        "type": "getter",
                        "location": "meyda/getFeature",
                        "args": ["bass"],
                        "min": 0,
                        "max": 1,
                        "source": "meyda"
                    }
                },
            },
        });

        let mut store = Store::new();
        seed_scene(&mut store, scene);
        store
            .dispatch(
                "scenes/applyScene",
                json!({ "sceneId": "scene-components" }),
            )
            .expect("applyScene dispatches");

        let module = store
            .state()
            .modules
            .values()
            .find(|module| module.module_name == "Text")
            .expect("live Text module");
        let new_parent = module.dollar_props["fillColor"]["id"]
            .as_str()
            .expect("fresh parent input id")
            .to_string();
        assert_ne!(new_parent, "doc-fill", "fresh parent input id");

        // The live module owns the four component sub-inputs.
        for key in ["r", "g", "b", "a"] {
            let sub_id = format!("{new_parent}-{key}");
            let input = store
                .state()
                .inputs
                .get(&sub_id)
                .unwrap_or_else(|| panic!("sub-input {sub_id} minted"));
            assert_eq!(input.data["moduleId"], json!(module.id));
            assert_eq!(input.data["prop"], json!("fillColor"));
            assert_eq!(input.data["path"], json!(format!("[{key}]")));
        }

        // The saved red-component link was remapped onto the fresh id.
        let remapped = format!("{new_parent}-r");
        let link = store
            .state()
            .input_links
            .get(&remapped)
            .expect("the component link is restored on the fresh id");
        assert_eq!(link.source.as_deref(), Some("meyda"));
        assert!(
            !store.state().input_links.contains_key("doc-fill-r"),
            "the stale document id is not resurrected"
        );
    }

    #[test]
    fn apply_scene_normalises_numeric_enabled_flags() {
        // Legacy Scene documents store `enabled: 1` / `0`; the engine treats
        // Modules as drawable only for boolean `true`, so a restored Webcam
        // used to be skipped (compositor.rs / input_links.rs strict check).
        let mut scene = two_row_scene("scene-numeric-enabled");
        scene["modules"]["doc-bands"]["meta"]["enabled"] = json!(1);
        scene["modules"]["doc-ball"]["meta"]["enabled"] = json!(0);

        let mut store = Store::new();
        seed_scene(&mut store, scene);
        store
            .dispatch(
                "scenes/applyScene",
                json!({ "sceneId": "scene-numeric-enabled" }),
            )
            .expect("applyScene dispatches");

        let module = |name: &str| {
            store
                .state()
                .modules
                .values()
                .find(|module| module.module_name == name)
                .expect("module exists")
        };
        assert_eq!(module("Bands").meta["enabled"], json!(true));
        assert_eq!(module("Ball").meta["enabled"], json!(false));
    }

    #[test]
    fn apply_scene_builds_rows_with_fresh_ids_and_merged_props() {
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-1"));
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-1" }))
            .expect("applyScene dispatches");

        assert_eq!(store.state().groups.len(), 2);
        let backgrounds: Vec<&Group> = store
            .state()
            .groups
            .iter()
            .filter(|group| group.name == "Background")
            .collect();
        assert_eq!(backgrounds.len(), 1);
        let background = backgrounds[0];
        assert_ne!(background.id, "doc-bg", "fresh Group id");
        assert_eq!(background.scene_id.as_deref(), Some("scene-1"));
        assert_eq!(background.scene_row, Some(0));
        assert_eq!(background.modules.len(), 1);

        let module_id = &background.modules[0];
        let module = store.state().modules.get(module_id).expect("module exists");
        assert_ne!(module_id, "doc-bands", "fresh Module id");
        assert_eq!(module.module_name, "Bands");
        // The snapshot prop is applied on top of the registry default (0.05).
        assert_eq!(module.props["u_ringWidth"], json!(0.3));
        assert_eq!(module.meta["enabled"], json!(true));

        let effects = store
            .state()
            .groups
            .iter()
            .find(|group| group.name == "Effects")
            .expect("Effects group");
        assert_eq!(effects.scene_row, Some(1));
        assert_eq!(effects.alpha, 0.5);
        assert_eq!(effects.composite_operation, "screen");
        // `inheritFrom` was remapped to the live Background id.
        assert_eq!(
            effects.inherit_from,
            InheritFrom::Group(background.id.clone())
        );
    }

    #[test]
    fn load_scene_from_json_applies_by_default_and_skips_it_when_asked() {
        // `scenes.js:2048-2074` defaults `apply` to `true`: opening a Scene
        // JSON (File → Open Scene, the worker's `loadPreset` branch) applies
        // it, so the native action must do the same.
        let mut store = Store::new();
        let opened = store
            .dispatch(
                "scenes/loadSceneFromJson",
                json!({ "scene": two_row_scene("scene-opened") }),
            )
            .expect("loadSceneFromJson dispatches");
        assert_eq!(opened["id"], json!("scene-opened"));
        assert_eq!(
            store.scenes_state().performance["currentSceneId"],
            json!("scene-opened"),
            "the Scene is applied, not just upserted"
        );
        assert_eq!(store.applied_scene_id.as_deref(), Some("scene-opened"));
        let names: Vec<&str> = store
            .state()
            .groups
            .iter()
            .map(|group| group.name.as_str())
            .collect();
        assert_eq!(names, ["Background", "Effects"]);
        assert!(
            store
                .state()
                .modules
                .values()
                .any(|module| module.module_name == "Bands"),
            "the Scene's Modules are live"
        );

        // Imports and seeding upsert only: an explicit `apply: false` must
        // leave the live stack and the applied Scene alone.
        let mut store = Store::new();
        store
            .dispatch(
                "scenes/loadSceneFromJson",
                json!({ "scene": two_row_scene("scene-imported"), "apply": false }),
            )
            .expect("loadSceneFromJson dispatches");
        assert!(
            store
                .scenes_state()
                .scenes
                .iter()
                .any(|scene| scene["id"] == json!("scene-imported")),
            "the Scene is still upserted"
        );
        assert!(store.state().groups.is_empty(), "apply:false is load-only");
        assert_eq!(
            store.scenes_state().performance["currentSceneId"],
            json!(null)
        );
        assert!(store.applied_scene_id.is_none());
    }

    #[test]
    fn apply_scene_materialises_point10_modules() {
        let mut store = Store::new();
        seed_scene(
            &mut store,
            json!({
                "id": "scene-point10",
                "name": "Point10 scene",
                "groups": [{
                    "id": "doc-p10",
                    "name": "Effects",
                    "hidden": false,
                    "modules": ["doc-waveform"],
                    "enabled": 1,
                    "alpha": 1,
                    "inherit": true,
                    "inheritFrom": -1,
                    "pipeline": false,
                    "clearing": true,
                    "compositeOperation": "normal",
                }],
                "modules": {
                    "doc-waveform": {
                        "$id": "doc-waveform",
                        "$moduleName": "Point10 Waveform",
                        "meta": { "name": "Point10 Waveform", "enabled": true },
                        "props": { "opacity": 4.0, "speed": 2.0 },
                        "data": {},
                        "$props": {},
                    },
                },
            }),
        );
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-point10" }))
            .expect("applyScene dispatches");

        let group = store
            .state()
            .groups
            .iter()
            .find(|group| group.name == "Effects")
            .expect("Effects group");
        let module_id = &group.modules[0];
        let module = store.state().modules.get(module_id).expect("module exists");
        assert_eq!(module.module_name, "Point10 Waveform");
        assert_eq!(module.status, Vec::<Value>::new(), "registered, no error");
        // The registry meta survives the apply (the snapshot merges on top).
        assert_eq!(module.meta["type"], json!("2d"));
        assert_eq!(module.meta["category"], json!("3d-scenes"));
        assert_eq!(module.meta["author"], json!("Point10"));
        // Scene props are coerced against the Point10 schema: `opacity` is
        // strict-clamped to its 0..1 range, `speed` keeps its saved value and
        // the untouched levers keep their defaults.
        assert_eq!(module.props["opacity"], json!(1.0));
        assert_eq!(module.props["speed"], json!(2.0));
        assert_eq!(module.props["gain"], json!(1.0));
        assert_eq!(module.props["height"], json!(1.0));
    }

    #[test]
    fn apply_scene_materialises_isf_modules_from_the_corpus() {
        let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/isf");
        if !corpus.is_dir() {
            eprintln!("skipping ISF Scene test: {} is missing", corpus.display());
            return;
        }
        let mut store = Store::with_state_path_and_isf(None, Some(&corpus));
        seed_scene(
            &mut store,
            json!({
                "id": "scene-isf",
                "name": "ISF scene",
                "groups": [{
                    "id": "doc-isf",
                    "name": "Effects",
                    "hidden": false,
                    "modules": ["doc-bloom"],
                    "enabled": 1,
                    "alpha": 1,
                    "inherit": true,
                    "inheritFrom": -1,
                    "pipeline": false,
                    "clearing": true,
                    "compositeOperation": "normal",
                }],
                "modules": {
                    "doc-bloom": {
                        "$id": "doc-bloom",
                        "$moduleName": "Bloom",
                        "meta": { "name": "Bloom", "enabled": true },
                        "props": { "blurAmount": 6.0 },
                        "data": {},
                        "$props": {},
                    },
                },
            }),
        );
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-isf" }))
            .expect("applyScene dispatches");

        let group = store
            .state()
            .groups
            .iter()
            .find(|group| group.name == "Effects")
            .expect("Effects group");
        let module_id = &group.modules[0];
        let module = store.state().modules.get(module_id).expect("module exists");
        assert_eq!(module.module_name, "Bloom");
        assert_eq!(module.status, Vec::<Value>::new(), "registered, no error");
        // The registry meta survives the apply; the snapshot prop overrides the
        // default and the other declared prop keeps its header default.
        assert_eq!(module.meta["type"], json!("isf"));
        assert_eq!(module.meta["author"], json!("by VIDVOX"));
        assert_eq!(module.props["blurAmount"], json!(6.0));
        assert_eq!(module.props["intensity"], json!(0));
    }

    #[test]
    fn apply_scene_replaces_previous_groups_and_keeps_gallery_and_non_scene() {
        let mut store = Store::new();
        // This test asserts the replacement itself, so switch instantly rather
        // than letting the default Fade keep the old Groups as transition
        // ghosts for a second.
        store
            .dispatch(
                "scenes/updatePerformance",
                json!({ "config": { "allowedTransitions": ["None"] } }),
            )
            .expect("performance config");
        store
            .dispatch(
                "groups/createGroup",
                json!({ "id": "gallery", "name": GALLERY_GROUP_NAME }),
            )
            .expect("gallery");
        store
            .dispatch(
                "groups/createGroup",
                json!({ "id": "free", "name": "My Free Group" }),
            )
            .expect("free group");

        seed_scene(&mut store, two_row_scene("scene-1"));
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-1" }))
            .expect("apply");
        let first_ids: Vec<String> = store
            .state()
            .groups
            .iter()
            .filter(|group| group.scene_id.is_some())
            .map(|group| group.id.clone())
            .collect();
        assert_eq!(first_ids.len(), 2);

        let mut second = two_row_scene("scene-2");
        second["name"] = json!("Second");
        seed_scene(&mut store, second);
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-2" }))
            .expect("apply second");

        let ids: Vec<&str> = store
            .state()
            .groups
            .iter()
            .map(|group| group.id.as_str())
            .collect();
        assert!(ids.contains(&"gallery"), "Gallery preserved");
        assert!(ids.contains(&"free"), "non-Scene group preserved");
        for old in first_ids {
            assert!(!ids.contains(&old.as_str()), "previous Scene group removed");
        }
        let stamped: Vec<&Group> = store
            .state()
            .groups
            .iter()
            .filter(|group| group.scene_id.as_deref() == Some("scene-2"))
            .collect();
        assert_eq!(stamped.len(), 2);
    }

    #[test]
    fn apply_scene_unknown_module_still_creates_an_instance_with_status() {
        let mut store = Store::new();
        let scene = json!({
            "id": "scene-missing",
            "name": "Missing",
            "groups": [{
                "id": "doc-g",
                "name": "Effects",
                "hidden": false,
                "modules": ["doc-missing"],
                "enabled": 1,
                "alpha": 1,
                "inherit": true,
                "inheritFrom": -1,
                "pipeline": false,
                "clearing": false,
                "compositeOperation": "normal",
            }],
            "modules": {
                "doc-missing": {
                    "$id": "doc-missing",
                    "$moduleName": "Totally Not A Module",
                    "meta": { "name": "Totally Not A Module" },
                    "props": { "amount": 1 },
                    "data": {},
                    "$props": {},
                },
            },
        });
        seed_scene(&mut store, scene);
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-missing" }))
            .expect("apply");

        let group = &store.state().groups[0];
        let module = store
            .state()
            .modules
            .get(&group.modules[0])
            .expect("unknown module is still active");
        assert_eq!(module.module_name, "Totally Not A Module");
        assert_eq!(module.status.len(), 1);
        assert_eq!(module.status[0]["type"], json!("error"));
        assert_eq!(module.props["amount"], json!(1));
    }

    #[test]
    fn apply_scene_null_row_holes_leave_the_row_untouched() {
        let mut store = Store::new();
        store
            .dispatch(
                "groups/createGroup",
                json!({ "id": "free", "name": "Free" }),
            )
            .expect("free group");
        let scene = json!({
            "id": "scene-holes",
            "name": "Holes",
            "groups": [
                null,
                {
                    "id": "doc-fx",
                    "name": "Effects",
                    "hidden": false,
                    "modules": [],
                    "enabled": 1,
                    "alpha": 1,
                    "inherit": true,
                    "inheritFrom": -1,
                    "pipeline": false,
                    "clearing": false,
                    "compositeOperation": "normal",
                },
            ],
            "modules": {},
        });
        seed_scene(&mut store, scene);
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-holes" }))
            .expect("apply");

        // The row-0 hole means no new Group exists for row 0; the non-Scene
        // Group is untouched and the Effects group lands on row 1.
        let ids: Vec<&str> = store
            .state()
            .groups
            .iter()
            .map(|group| group.id.as_str())
            .collect();
        assert!(ids.contains(&"free"));
        let effects = store
            .state()
            .groups
            .iter()
            .find(|group| group.name == "Effects")
            .expect("Effects group");
        assert_eq!(effects.scene_row, Some(1));
        assert!(
            store
                .state()
                .groups
                .iter()
                .all(|group| group.name == "Free" || group.name == "Effects")
        );
    }

    #[test]
    fn apply_scene_emits_the_mirror_mutations() {
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-deltas"));
        store.drain_deltas();
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-deltas" }))
            .expect("apply");
        let types = delta_types(&mut store);

        for expected in [
            "groups/ADD_GROUP",
            "modules/ADD_ACTIVE_MODULE",
            "groups/ADD_MODULE_TO_GROUP",
            "inputs/ADD_INPUT",
            "groups/REPLACE_GROUPS",
            "scenes/UPDATE_PERFORMANCE",
            "scenes/SET_ACTIVE_ROWS",
        ] {
            assert!(
                types.iter().any(|type_| type_ == expected),
                "missing delta {expected} in {types:?}"
            );
        }
    }

    #[test]
    fn a_link_saved_against_a_replaced_scene_input_is_retargeted() {
        // A Scene apply mints fresh input ids (`scenes.js` does the same), so a
        // link editor or Tween control that was opened before the apply still
        // holds the previous live id. Saving its link must not be dropped:
        // the control it names was re-created by the apply.
        let mut store = Store::new();
        // Switch instantly so the replaced Groups' inputs are actually gone.
        store
            .dispatch(
                "scenes/updatePerformance",
                json!({ "config": { "allowedTransitions": ["None"] } }),
            )
            .expect("performance config");
        let scene = json!({
            "id": "scene-retarget",
            "name": "Retarget Scene",
            "groups": [{
                "id": "doc-group",
                "name": "Effects",
                "hidden": false,
                "modules": ["doc-bands"],
                "enabled": 1,
                "alpha": 1,
                "inherit": true,
                "inheritFrom": -1,
                "pipeline": false,
                "clearing": true,
                "compositeOperation": "normal",
            }],
            "modules": {
                "doc-bands": {
                    "$id": "doc-bands",
                    "$moduleName": "Bands",
                    "meta": { "name": "Bands", "enabled": true },
                    "props": { "u_ringWidth": 0.3 },
                    "data": {},
                    "$props": { "u_ringWidth": { "id": "doc-width", "type": "float" } },
                },
            },
            "inputs": {
                "focusedInput": { "id": null, "title": null },
                "inputs": {
                    "doc-width": {
                        "id": "doc-width",
                        "type": "action",
                        "data": { "moduleId": "doc-bands", "prop": "u_ringWidth" },
                        "location": "modules/updateProp",
                    },
                },
                "inputLinks": {
                    "doc-width": {
                        "id": "doc-width",
                        "type": "getter",
                        "location": "meyda/getFeature",
                        "args": ["bass"],
                        "source": "meyda",
                        "min": 0.5,
                        "max": 1.5,
                    },
                },
            },
            "expressions": { "assignments": {} },
            "tweens": [],
            "midi": {},
        });
        seed_scene(&mut store, scene);
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-retarget" }))
            .expect("first apply");

        let live = |store: &Store| -> (String, String) {
            let group = store
                .state()
                .groups
                .iter()
                .find(|group| group.name == "Effects")
                .expect("Effects group");
            let module_id = group.modules.first().expect("Bands module").clone();
            let input_id = store.state().modules[&module_id].dollar_props["u_ringWidth"]["id"]
                .as_str()
                .expect("prop input id")
                .to_string();
            (module_id, input_id)
        };
        let (_, stale_input) = live(&store);
        assert!(
            store.state().input_links.contains_key(&stale_input),
            "the Scene link is live after the first apply"
        );

        // Re-apply the Scene: every Group/Module/Input id is rebuilt.
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-retarget" }))
            .expect("second apply");
        let (module_id, rebuilt_input) = live(&store);
        assert_ne!(stale_input, rebuilt_input, "the apply mints a new input id");
        assert!(
            !store.state().inputs.contains_key(&stale_input),
            "the old live input is gone"
        );

        // The editor's Save still names the replaced input.
        let saved = store
            .dispatch(
                "inputs/createInputLink",
                json!({
                    "inputId": stale_input,
                    "type": "getter",
                    "location": "meyda/getFeature",
                    "args": ["bass"],
                    "source": "meyda",
                    "min": 0.1,
                    "max": 0.2,
                }),
            )
            .expect("the stale link save dispatches");
        assert_eq!(saved, json!(true), "the replaced input is retargeted");

        let link = store
            .state()
            .input_links
            .get(&rebuilt_input)
            .expect("the link lands on the rebuilt input");
        assert_eq!(link.source.as_deref(), Some("meyda"));
        assert_eq!(link.min, Some(0.1));
        assert_eq!(link.max, Some(0.2));
        assert!(
            !store.state().input_links.contains_key(&stale_input),
            "nothing is stored under the replaced id"
        );

        // The retargeted link still drives the rebuilt module's prop.
        store.update_input_links(
            &AudioFrame {
                bass: 0.5,
                ..AudioFrame::default()
            },
            Duration::ZERO,
        );
        let value = store.state().modules[&module_id].props["u_ringWidth"]
            .as_f64()
            .expect("u_ringWidth");
        assert!(
            (value - 0.15).abs() < 1e-6,
            "0.5 bass mapped to [0.1, 0.2] writes 0.15, got {value}"
        );

        // A tween link follows its rebuilt input too; the saved location must
        // point at the new tween id rather than the retired one.
        assert_eq!(
            store
                .dispatch(
                    "inputs/createInputLink",
                    json!({
                        "inputId": stale_input,
                        "type": "state",
                        "location": format!("tweens.tweens['{stale_input}'].value[0]"),
                        "source": "tween",
                        "min": 0,
                        "max": 1,
                    }),
                )
                .expect("the stale tween link save dispatches"),
            json!(true)
        );
        let link = store
            .state()
            .input_links
            .get(&rebuilt_input)
            .expect("the tween link lands on the rebuilt input");
        assert_eq!(link.source.as_deref(), Some("tween"));
        assert_eq!(
            link.location.as_deref(),
            Some(format!("tweens.tweens['{rebuilt_input}'].value[0]").as_str())
        );
    }

    #[test]
    fn apply_scene_creates_the_saved_input_links() {
        let mut store = Store::new();
        // Build a default Scene document, whose group/module input ids are
        // real, and apply it.
        let definition = modv_engine::default_scenes::get_default_definition("Polygon Grid")
            .expect("default definition");
        let mut skipped = 0usize;
        let document =
            build_default_scene_document(&store.registry, definition, &json!({}), &mut skipped)
                .document;
        let document_links = document["inputs"]["inputLinks"]
            .as_object()
            .expect("built links")
            .clone();
        assert!(
            !document_links.is_empty(),
            "a default Scene carries default audio links"
        );

        seed_scene(&mut store, document.clone());
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": document["id"] }))
            .expect("apply");

        assert!(
            !store.state().input_links.is_empty(),
            "applying a Scene creates its input links"
        );
        let has_bass = store.state().input_links.values().any(|link| {
            link.source.as_deref() == Some("meyda")
                && link.location.as_deref() == Some("meyda/getFeature")
        });
        assert!(has_bass, "the automatic bass link is created");
        let has_tween = store
            .state()
            .input_links
            .values()
            .any(|link| link.source.as_deref() == Some("tween"));
        assert!(has_tween, "the automatic BPM tween link is created");
    }

    #[test]
    fn default_scene_links_keep_their_declared_smoothing() {
        // `createBassLink` (`scenes.js:758-789`) materialises a declared
        // smoothing as `args: [feature, smoothingId, value]` after minting the
        // id through `meyda/getSmoothingId`. The engine parsed the value but
        // never forwarded it, so the v15 scenes' `smoothing: 0.6` links fed
        // raw bass in engine mode.
        //
        // Every current smoothing link targets an ISF corpus shader, so the
        // store needs the corpus registry for the links to resolve.
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/isf");
        if !corpus.is_dir() {
            eprintln!(
                "skipping default-scene smoothing test: ISF corpus missing at {}",
                corpus.display()
            );
            return;
        }
        let mut store = Store::with_state_path_and_isf(None, Some(&corpus));
        store
            .dispatch("scenes/ensureDefaultScenes", json!({}))
            .expect("seeding");

        let mut checked = 0usize;
        for definition in modv_engine::default_scenes::default_scenes() {
            for link in &definition.input_links {
                let Some(amount) = link.smoothing else {
                    continue;
                };
                if link.source.as_deref() == Some("tween") {
                    continue;
                }
                let scene = store
                    .scenes_state()
                    .scenes
                    .iter()
                    .find(|scene| scene["name"] == json!(definition.name))
                    .unwrap_or_else(|| panic!("{} is seeded", definition.name));
                let input_id = scene_link_input_id(scene, link);
                let built = &scene["inputs"]["inputLinks"][input_id.as_str()];
                let args = built["args"].as_array().expect("link args");
                assert_eq!(
                    args.len(),
                    3,
                    "a declared smoothing emits the args triple: {built}"
                );
                assert_eq!(
                    args[0],
                    json!(link.feature.clone().unwrap_or_else(|| "bass".to_string()))
                );
                let smoothing_id = args[1].as_str().expect("smoothing id");
                assert_eq!(smoothing_id.len(), 36, "the id is a uuid: {built}");
                assert_eq!(
                    args[2].as_f64(),
                    Some(amount),
                    "the declared smoothing amount travels: {built}"
                );
                assert_eq!(
                    store.link_smoothing_value(smoothing_id),
                    Some(0.0),
                    "the minted id is registered in the smoothing registry"
                );
                checked += 1;
            }
        }
        assert!(checked >= 3, "the v15 scenes declare smoothing links");

        // A link without a declared smoothing keeps the single-arg shape.
        let plain = store
            .scenes_state()
            .scenes
            .iter()
            .flat_map(|scene| {
                scene["inputs"]["inputLinks"]
                    .as_object()
                    .map(|links| links.values().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            })
            .find(|link| {
                link["location"] == json!("meyda/getFeature")
                    && link["args"].as_array().is_some_and(|args| args.len() == 1)
            });
        assert!(
            plain.is_some(),
            "an automatic bass link still emits args: [feature]"
        );
    }

    #[test]
    fn re_applying_a_scene_keeps_tween_playheads_and_prunes_unknown_ids() {
        let mut store = Store::new();
        let definition = modv_engine::default_scenes::get_default_definition("Polygon Grid")
            .expect("default definition");
        let mut skipped = 0usize;
        let document =
            build_default_scene_document(&store.registry, definition, &json!({}), &mut skipped)
                .document;

        seed_scene(&mut store, document.clone());
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": document["id"] }))
            .expect("first apply");

        // The apply materialised the document's scenes' tweens in the runtime.
        let tween_id = store
            .state()
            .tweens
            .as_array()
            .and_then(|tweens| tweens.first())
            .and_then(|tween| tween.get("id"))
            .and_then(Value::as_str)
            .expect("the Scene carries a tween")
            .to_string();
        store
            .tweens
            .get_mut(&tween_id)
            .expect("the tween is in the runtime")
            .set_frame(4);

        // A runtime tween that `state.tweens` no longer lists must be dropped.
        store
            .tweens
            .upsert_json(
                &json!({"id": "stale", "data": [[0], [1]], "useBpm": false}),
                store.fps,
                store.bpm(),
            )
            .expect("stale tween registers");
        assert!(store.tweens.contains("stale"));

        // A second apply re-runs `restoreSceneInputs`: the surviving tween must
        // keep its playhead (a binding rebuild reset every tween to frame 0,
        // so multi-row Scene advances snapped linked props back to the start).
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": document["id"] }))
            .expect("second apply");

        assert!(
            store.tweens.contains(&tween_id),
            "the surviving tween is still in the runtime"
        );
        assert_eq!(
            store.tweens.get(&tween_id).expect("tween").frame(),
            4,
            "a Scene apply must not reset a surviving tween's playhead"
        );
        assert!(
            !store.tweens.contains("stale"),
            "ids that left state.tweens are pruned"
        );
    }

    #[test]
    fn removing_a_scene_finishes_its_in_flight_row_transition_first() {
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-a"));
        seed_scene(&mut store, two_row_scene("scene-b"));
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-a" }))
            .expect("apply a");
        store
            .dispatch("scenes/startPerformance", json!({}))
            .expect("start performance");

        store
            .apply_scene_row_transition("scene-b", 0)
            .expect("row transition runs")
            .expect("scene-b has a row 0");
        assert!(
            store.row_transition(0).is_some(),
            "the crossfade keeps scene-a's Group alive as a ghost"
        );

        store
            .dispatch("scenes/removeScene", json!({ "sceneId": "scene-a" }))
            .expect("remove");

        // Deleting the Scene deleted the Groups its ghost referenced; the
        // transition must be finished (not left pointing at dead ids), and no
        // ghost may survive (`scenes.js:2818-2819`).
        assert!(
            store.row_transition(0).is_none(),
            "no transition references the removed Scene any more"
        );
        assert!(
            !store
                .state()
                .groups
                .iter()
                .any(|group| group.transition_ghost == Some(true)),
            "no ghost Group survives the removal"
        );
        // A tick after the removal must run without touching dead ids.
        store
            .dispatch("scenes/tick", json!({ "delta": 16.0 }))
            .expect("tick after removal");
    }

    // ————————————————— default seeding —————————————————

    #[test]
    fn default_seeding_seeds_the_full_set_once() {
        let dir = temp_dir();
        let state_path = dir.join("engine-state.json");
        let mut store = Store::with_state_path(Some(&state_path));
        store.drain_deltas();

        store
            .dispatch("scenes/ensureDefaultScenes", json!({}))
            .expect("seeding");
        let total = modv_engine::default_scenes::default_scenes().len();
        assert_eq!(store.scenes_state().scenes.len(), total);
        assert_eq!(store.scenes_state().defaults_version, DEFAULTS_VERSION);
        assert!(store.scenes_state().defaults_seeded);
        assert_eq!(store.scenes_state().seeded_default_scenes.len(), total);
        assert!(dir.join("scenes.json").exists(), "scenes.json persisted");

        let types = delta_types(&mut store);
        for expected in [
            "scenes/REPLACE_SCENES",
            "scenes/SET_SEEDED_DEFAULT_SCENES",
            "scenes/SET_DEFAULTS_SEEDED",
            "scenes/SET_DEFAULTS_VERSION",
        ] {
            assert!(types.iter().any(|type_| type_ == expected), "{types:?}");
        }

        // A second pass seeds nothing.
        store
            .dispatch("scenes/ensureDefaultScenes", json!({}))
            .expect("second seeding");
        assert_eq!(store.scenes_state().scenes.len(), total);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_seeding_with_a_newer_version_seeds_only_the_new_batch() {
        let mut store = Store::new();
        // Versions 10-12 only modify existing Scenes; the newest ADDING batch
        // is version 9, so seed from version 8 to observe a partial seed.
        store.scenes.defaults_version = 8;
        store
            .dispatch("scenes/ensureDefaultScenes", json!({}))
            .expect("seeding");

        let expected = modv_engine::default_scenes::default_scenes()
            .iter()
            .filter(|scene| scene.introduced_version > 8)
            .count();
        assert!(expected > 0, "the default data has a version-9 batch");
        assert_eq!(store.scenes_state().scenes.len(), expected);
        assert_eq!(store.scenes_state().defaults_version, DEFAULTS_VERSION);
    }

    #[test]
    fn default_seeding_refreshes_the_version_13_scenes_in_place() {
        // The defaults version bump is refresh-only: an install on v12 keeps
        // its Scene ids/timing but receives the edited payload (no CMYK
        // Halftone in Halftone Harbor, MBOX3 hue fixed at 0 with its tween
        // gone). Mirrors `getDefaultScenesToReplace` + `ADD_SCENE` in
        // `scenes.js`.
        let mut store = Store::new();
        let cmyk = json!({
            "id": "scene-halftone",
            "name": "Halftone Harbor",
            "enabled": true,
            "minSeconds": 8,
            "maxSeconds": 21,
            "groups": [
                {
                    "id": "doc-effects",
                    "name": "Effects",
                    "hidden": false,
                    "modules": ["doc-cmyk"],
                    "enabled": 1,
                    "alpha": 1,
                    "inherit": true,
                    "inheritFrom": -1,
                    "pipeline": false,
                    "clearing": true,
                    "compositeOperation": "normal",
                }
            ],
            "modules": {
                "doc-cmyk": {
                    "$id": "doc-cmyk",
                    "$moduleName": "CMYK Halftone",
                    "meta": { "name": "CMYK Halftone", "enabled": true },
                    "props": { "gridSize": 45 },
                    "data": {},
                    "$props": {},
                }
            },
            "inputs": { "inputs": {}, "inputLinks": {} },
            "expressions": { "assignments": {} },
            "tweens": [],
        });
        let mbox = json!({
            "id": "scene-mbox",
            "name": "MBOX Light Tunnel",
            "enabled": true,
            "minSeconds": 10,
            "maxSeconds": 28,
            "groups": [
                {
                    "id": "doc-bg",
                    "name": "Background",
                    "hidden": false,
                    "modules": ["doc-mbox"],
                    "enabled": 1,
                    "alpha": 1,
                    "inherit": false,
                    "inheritFrom": -1,
                    "pipeline": false,
                    "clearing": true,
                    "compositeOperation": "normal",
                }
            ],
            "modules": {
                "doc-mbox": {
                    "$id": "doc-mbox",
                    "$moduleName": "MBOX3",
                    "meta": { "name": "MBOX3", "enabled": true },
                    "props": { "dhue": 0.25 },
                    "data": {},
                    "$props": {},
                }
            },
            "inputs": {
                "inputs": {},
                "inputLinks": { "link-dhue": { "control": "dhue" } }
            },
            "expressions": { "assignments": {} },
            "tweens": [],
        });
        store.scenes.scenes = vec![cmyk, mbox];
        store.scenes.defaults_version = DEFAULTS_VERSION - 1;
        store.scenes.seeded_default_scenes = vec![
            "Halftone Harbor".to_string(),
            "MBOX Light Tunnel".to_string(),
        ];

        store
            .dispatch("scenes/ensureDefaultScenes", json!({}))
            .expect("seeding");

        let halftone = find_default_scene(&store, "Halftone Harbor");
        assert_eq!(halftone["id"], json!("scene-halftone"), "id preserved");
        assert_eq!(halftone["minSeconds"], json!(8), "timing preserved");
        // The stale payload is gone: the saved CMYK Halftone module id no
        // longer exists and the rebuilt groups carry fresh ids. (The test
        // registry has no ISF corpus, so the definition's own modules are
        // skipped here; the v13 payload itself is pinned in the engine
        // crate's `default_scenes` tests.)
        assert!(
            !halftone["modules"]
                .as_object()
                .expect("modules")
                .contains_key("doc-cmyk"),
            "the saved CMYK Halftone payload survived: {}",
            halftone["modules"]
        );
        assert!(
            !halftone["groups"]
                .as_array()
                .expect("groups")
                .iter()
                .any(|group| group["id"] == json!("doc-effects")),
            "the saved groups were not rebuilt: {}",
            halftone["groups"]
        );

        let mbox = find_default_scene(&store, "MBOX Light Tunnel");
        assert_eq!(mbox["id"], json!("scene-mbox"), "id preserved");
        assert!(
            !mbox["modules"]
                .as_object()
                .expect("modules")
                .contains_key("doc-mbox"),
            "the saved MBOX3 payload survived: {}",
            mbox["modules"]
        );
        assert!(
            !mbox["inputs"]["inputLinks"]
                .as_object()
                .expect("links")
                .contains_key("link-dhue"),
            "the stale dhue link survived: {}",
            mbox["inputs"]["inputLinks"]
        );

        // A second pass must not re-refresh (the version is already current).
        store
            .dispatch("scenes/ensureDefaultScenes", json!({}))
            .expect("second seeding");
        assert_eq!(
            find_default_scene(&store, "Halftone Harbor")["id"],
            json!("scene-halftone")
        );
    }

    #[test]
    fn default_seeding_retires_strobe_riot_for_version_17_installs() {
        let mut store = Store::new();
        store.scenes.scenes = vec![json!({
            "id": "scene-strobe",
            "name": "Strobe Riot",
            "enabled": true,
            "minSeconds": 7,
            "maxSeconds": 18,
            "groups": [],
            "modules": {},
            "inputs": { "inputs": {}, "inputLinks": {} },
            "expressions": { "assignments": {} },
            "tweens": [],
        })];
        store.scenes.defaults_version = DEFAULTS_VERSION - 1;
        store.scenes.seeded_default_scenes = vec!["Strobe Riot".to_string()];

        store
            .dispatch("scenes/ensureDefaultScenes", json!({}))
            .expect("seeding");

        assert!(
            store.scenes_state().scenes.is_empty(),
            "the seeded Strobe Riot Scene was not retired"
        );
        assert!(
            !store
                .scenes_state()
                .seeded_default_scenes
                .iter()
                .any(|name| name == "Strobe Riot"),
            "Strobe Riot is still marked seeded"
        );
        assert_eq!(store.scenes_state().defaults_version, DEFAULTS_VERSION);
    }

    #[test]
    fn default_seeding_keeps_an_unseeded_strobe_riot_scene() {
        let mut store = Store::new();
        store.scenes.scenes = vec![json!({
            "id": "scene-user-strobe",
            "name": "Strobe Riot",
            "enabled": true,
            "groups": [],
            "modules": {},
            "inputs": { "inputs": {}, "inputLinks": {} },
            "expressions": { "assignments": {} },
            "tweens": [],
        })];
        store.scenes.defaults_version = DEFAULTS_VERSION - 1;

        store
            .dispatch("scenes/ensureDefaultScenes", json!({}))
            .expect("seeding");

        assert_eq!(
            store.scenes_state().scenes.len(),
            1,
            "a custom Scene must survive the retirement"
        );
        assert_eq!(
            store.scenes_state().scenes[0]["id"],
            json!("scene-user-strobe")
        );
    }

    // ————————————————— persistence —————————————————

    #[test]
    fn scenes_json_round_trips_and_tolerates_unknown_keys() {
        let dir = temp_dir();
        let state_path = dir.join("engine-state.json");
        let mut store = Store::with_state_path(Some(&state_path));
        store
            .dispatch("scenes/ensureDefaultScenes", json!({}))
            .expect("seeding");
        store
            .dispatch(
                "scenes/updatePerformance",
                json!({ "config": { "rowNames": ["Background", "Effects"] } }),
            )
            .expect("performance");
        let expected_scenes = store.scenes_state().scenes.clone();
        let expected_names = store.scenes_state().seeded_default_scenes.clone();

        let file = dir.join("scenes.json");
        let mut document: Value =
            serde_json::from_slice(&fs::read(&file).expect("scenes.json")).expect("json");
        document["legacyTopLevel"] = json!(true);
        document["scenes"][0]["legacySceneKey"] = json!("kept");
        fs::write(&file, serde_json::to_vec(&document).unwrap()).expect("rewrite");

        let mut restored = Store::with_state_path(Some(&state_path));
        restored
            .dispatch("scenes/hydrateScenes", json!({}))
            .expect("hydrate");
        assert_eq!(restored.scenes_state().scenes.len(), expected_scenes.len());
        assert_eq!(
            restored.scenes_state().seeded_default_scenes,
            expected_names
        );
        assert_eq!(
            restored.scenes_state().performance["rowNames"],
            json!(["Background", "Effects"])
        );
        assert_eq!(
            restored.scenes_state().scenes[0]["legacySceneKey"],
            json!("kept")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn persist_writes_atomically_without_leaving_a_tmp_file() {
        let dir = temp_dir();
        let state_path = dir.join("engine-state.json");
        let mut store = Store::with_state_path(Some(&state_path));
        store
            .dispatch(
                "scenes/loadSceneFromJson",
                json!({ "scene": two_row_scene("scene-a"), "apply": false }),
            )
            .expect("load");

        assert!(dir.join("scenes.json").exists());
        assert!(
            !dir.join("scenes.json.tmp").exists(),
            "tmp file is renamed over the target"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hydrate_of_a_missing_file_leaves_the_list_empty() {
        let dir = temp_dir();
        let state_path = dir.join("engine-state.json");
        let mut store = Store::with_state_path(Some(&state_path));
        store
            .dispatch("scenes/hydrateScenes", json!({}))
            .expect("hydrate");
        assert!(store.scenes_state().scenes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    // ————————————————— playlist —————————————————

    #[test]
    fn export_import_playlist_round_trips() {
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-a"));
        store
            .dispatch(
                "scenes/updatePerformance",
                json!({ "config": { "order": "random" } }),
            )
            .expect("performance");
        let playlist = store
            .dispatch("scenes/exportPlaylist", json!({}))
            .expect("export");
        assert_eq!(playlist["type"], json!("modv-scenes-playlist"));
        assert_eq!(playlist["scenes"].as_array().unwrap().len(), 1);
        assert_eq!(playlist["performance"]["order"], json!("random"));

        let mut other = Store::new();
        other
            .dispatch("scenes/importPlaylist", json!({ "playlist": playlist }))
            .expect("import");
        assert_eq!(other.scenes_state().scenes.len(), 1);
        assert_eq!(other.scenes_state().scenes[0]["id"], json!("scene-a"));
        assert_eq!(other.scenes_state().performance["order"], json!("random"));
    }

    #[test]
    fn import_playlist_replace_mode_replaces_the_list() {
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-a"));
        let playlist = json!({
            "type": "modv-scenes-playlist",
            "version": 1,
            "scenes": [two_row_scene("scene-b")],
            "performance": {},
        });
        store
            .dispatch(
                "scenes/importPlaylist",
                json!({ "playlist": playlist, "mode": "replace" }),
            )
            .expect("import");
        assert_eq!(store.scenes_state().scenes.len(), 1);
        assert_eq!(store.scenes_state().scenes[0]["id"], json!("scene-b"));
    }

    // ————————————————— capture —————————————————

    #[test]
    fn capture_live_state_updates_the_scene_and_refuses_during_performance() {
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-capture"));
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-capture" }))
            .expect("apply");

        // Rename the live Background group, then capture.
        let background_id = store
            .state()
            .groups
            .iter()
            .find(|group| group.name == "Background")
            .map(|group| group.id.clone())
            .expect("background");
        store
            .dispatch(
                "groups/UPDATE_GROUP_BY_KEY",
                json!({ "groupId": background_id, "key": "name", "value": "Renamed BG" }),
            )
            .expect("rename");

        let captured = store
            .dispatch(
                "scenes/captureLiveStateToScene",
                json!({ "sceneId": "scene-capture" }),
            )
            .expect("capture");
        assert!(captured.get("error").is_none(), "{captured}");
        let groups = captured["scene"]["groups"].as_array().expect("groups");
        assert_eq!(groups[0]["name"], json!("Renamed BG"));
        assert!(
            captured["scene"]["modules"].as_object().unwrap().len() >= 2,
            "module snapshots captured"
        );

        store
            .dispatch("scenes/setPerformanceEnabled", json!({ "enabled": true }))
            .expect("enable");
        let refused = store
            .dispatch(
                "scenes/captureLiveStateToScene",
                json!({ "sceneId": "scene-capture" }),
            )
            .expect("capture refuses");
        assert_eq!(refused["error"], json!("performance"));
    }

    #[test]
    fn capture_of_an_unapplied_scene_is_refused() {
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-x"));
        let refused = store
            .dispatch(
                "scenes/captureLiveStateToScene",
                json!({ "sceneId": "scene-x" }),
            )
            .expect("capture");
        assert_eq!(refused["error"], json!("not-applied"));
    }

    // ————————————————— row names / performance —————————————————

    #[test]
    fn saved_row_names_survive_hydrate_rename_and_reload() {
        // Regression for the native Scenes grid losing the row labels: the
        // shared `scenes.json` carries `performance.rowNames` (the real install
        // uses ["WEBCAM", "Background", "Foreground", "Effects", ""]), and the
        // names must hydrate, rename, persist and reload unchanged.
        let dir = temp_dir();
        let state_path = dir.join("engine-state.json");
        let document = json!({
            "version": SCENES_STORAGE_VERSION,
            "defaultsSeeded": true,
            "defaultsVersion": DEFAULTS_VERSION,
            "seededDefaultScenes": [],
            "scenes": [two_row_scene("scene-a")],
            "performance": {
                "rowNames": ["WEBCAM", "Background", "Foreground", "Effects", ""],
            },
        });
        fs::write(
            dir.join("scenes.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .expect("write scenes.json");

        let mut store = Store::with_state_path(Some(&state_path));
        store
            .dispatch("scenes/hydrateScenes", json!({}))
            .expect("hydrate");
        assert_eq!(
            store.scenes_state().performance["rowNames"],
            json!(["WEBCAM", "Background", "Foreground", "Effects", ""]),
            "hydration restores the exact row names"
        );
        assert_eq!(
            store.row_names(),
            vec!["WEBCAM", "Background", "Foreground", "Effects", ""],
            "the grid mapping reads the stored names"
        );

        // Rename a row and append a new one, exactly like the row dialog.
        let renamed = [
            "WEBCAM",
            "Background",
            "Foreground",
            "Effects",
            "",
            "Overlay",
        ];
        store
            .dispatch("scenes/setRowNames", json!({ "rowNames": renamed }))
            .expect("rename rows");

        let saved: Value =
            serde_json::from_slice(&fs::read(dir.join("scenes.json")).expect("scenes.json"))
                .expect("json");
        assert_eq!(
            saved["performance"]["rowNames"],
            json!(renamed),
            "setRowNames persists immediately"
        );

        let mut reloaded = Store::with_state_path(Some(&state_path));
        reloaded
            .dispatch("scenes/hydrateScenes", json!({}))
            .expect("reload");
        assert_eq!(
            reloaded.row_names(),
            renamed
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            "the renamed rows survive a restart"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_cell_enabled_persists_and_manual_apply_still_uses_the_cell() {
        let dir = temp_dir();
        let state_path = dir.join("engine-state.json");
        let mut store = Store::with_state_path(Some(&state_path));
        seed_scene(&mut store, two_row_scene("scene-a"));
        seed_scene(&mut store, two_row_scene("scene-b"));

        let result = store
            .dispatch(
                "scenes/setCellEnabled",
                json!({ "sceneId": "scene-b", "rowIndex": 0, "enabled": false }),
            )
            .expect("disable cell");
        assert_eq!(result["enabled"], json!(false));
        assert_eq!(
            store.scenes_state().performance["disabledCells"],
            json!({ "scene-b": [0] })
        );
        let types = delta_types(&mut store);
        assert!(
            types.contains(&"scenes/UPDATE_PERFORMANCE".to_string()),
            "the mirror receives the new flags: {types:?}"
        );

        let saved: Value =
            serde_json::from_slice(&fs::read(dir.join("scenes.json")).expect("scenes.json"))
                .expect("json");
        assert_eq!(
            saved["performance"]["disabledCells"],
            json!({ "scene-b": [0] }),
            "the flag is stored in the shared scenes.json"
        );

        let mut reloaded = Store::with_state_path(Some(&state_path));
        reloaded
            .dispatch("scenes/hydrateScenes", json!({}))
            .expect("reload");
        assert_eq!(
            reloaded.scenes_state().performance["disabledCells"],
            json!({ "scene-b": [0] }),
            "the flag reloads from scenes.json"
        );

        // Performance mode skips the cell, but a manual apply still renders it.
        reloaded
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-b" }))
            .expect("manual apply");
        assert!(
            reloaded
                .state()
                .groups
                .iter()
                .any(|group| group.name == "Background" && group.scene_row == Some(0)),
            "a disabled cell is still part of a manual Scene apply"
        );

        // Unknown Scenes and rows are refused and never persisted.
        let missing = reloaded
            .dispatch(
                "scenes/setCellEnabled",
                json!({ "sceneId": "scene-missing", "rowIndex": 0, "enabled": false }),
            )
            .expect("unknown scene");
        assert_eq!(missing["updated"], json!(false));
        let empty = reloaded
            .dispatch(
                "scenes/setCellEnabled",
                json!({ "sceneId": "scene-b", "rowIndex": 99, "enabled": false }),
            )
            .expect("unknown row");
        assert_eq!(empty["updated"], json!(false));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_row_names_remaps_live_groups_and_persists() {
        let mut store = Store::new();
        store
            .dispatch(
                "groups/createGroup",
                json!({ "id": "bg", "name": "Background" }),
            )
            .expect("bg");
        store
            .dispatch(
                "groups/createGroup",
                json!({ "id": "fx", "name": "Effects" }),
            )
            .expect("fx");

        let names = store
            .dispatch(
                "scenes/setRowNames",
                json!({ "rowNames": ["Effects", "Background"] }),
            )
            .expect("row names");
        assert_eq!(names, json!(["Effects", "Background"]));
        assert_eq!(store.state().find_group("fx").unwrap().scene_row, Some(0));
        assert_eq!(store.state().find_group("bg").unwrap().scene_row, Some(1));
        assert_eq!(
            store.scenes_state().performance["rowNames"],
            json!(["Effects", "Background"])
        );
    }

    #[test]
    fn update_performance_normalises_timing_and_keeps_runtime_keys() {
        let mut store = Store::new();
        store
            .dispatch("scenes/setPerformanceEnabled", json!({ "enabled": true }))
            .expect("enable");
        store
            .dispatch(
                "scenes/updatePerformance",
                json!({ "config": { "minSeconds": 0.2, "maxSeconds": 2, "order": "random" } }),
            )
            .expect("update");
        let performance = &store.scenes_state().performance;
        assert_eq!(performance["minSeconds"], json!(1));
        assert_eq!(performance["maxSeconds"], json!(2));
        assert_eq!(performance["order"], json!("random"));
        assert_eq!(performance["enabled"], json!(true));
    }

    #[test]
    fn set_timing_unit_converts_globals_and_overrides_and_persists() {
        let dir = temp_dir();
        let state_path = dir.join("engine-state.json");
        let mut store = Store::with_state_path(Some(&state_path));
        seed_scene(&mut store, two_row_scene("scene-a"));
        seed_scene(&mut store, two_row_scene("scene-b"));
        store
            .dispatch(
                "scenes/updatePerformance",
                json!({
                    "config": {
                        "minSeconds": 8,
                        "maxSeconds": 24,
                        "transitionSeconds": 2,
                    }
                }),
            )
            .expect("performance config");
        store
            .dispatch("beats/SET_BPM", json!({ "bpm": 120 }))
            .expect("bpm");
        store
            .dispatch(
                "scenes/updateSceneTiming",
                json!({ "sceneId": "scene-a", "minSeconds": 1, "maxSeconds": 3 }),
            )
            .expect("scene override");
        store.drain_deltas();

        let config = store
            .dispatch("scenes/setTimingUnit", json!({ "unit": "bars" }))
            .expect("setTimingUnit");
        assert_eq!(config["timingUnit"], json!("bars"));
        assert_eq!(config["minSeconds"], json!(4), "8 s at 120 BPM is 4 bars");
        assert_eq!(config["maxSeconds"], json!(12));
        assert_eq!(
            store.scenes_state().scenes[0]["minSeconds"],
            json!(0.5),
            "the Scene override keeps its own minimum, in bars"
        );
        assert_eq!(store.scenes_state().scenes[0]["maxSeconds"], json!(1.5));
        assert_eq!(
            store.scenes_state().scenes[1]["minSeconds"],
            json!(8),
            "a Scene without an override is not rewritten"
        );

        let types = delta_types(&mut store);
        assert!(
            types.iter().any(|kind| kind == "scenes/UPDATE_PERFORMANCE"),
            "the performance config is mirrored: {types:?}"
        );
        assert!(
            types.iter().any(|kind| kind == "scenes/ADD_SCENE"),
            "the converted override is mirrored: {types:?}"
        );

        let saved: Value = serde_json::from_slice(
            &fs::read(dir.join("scenes.json")).expect("scenes.json is written"),
        )
        .expect("scenes.json is JSON");
        assert_eq!(saved["performance"]["timingUnit"], json!("bars"));
        assert_eq!(saved["performance"]["minSeconds"], json!(4));
        assert_eq!(saved["scenes"][0]["minSeconds"], json!(0.5));

        // Switching back at the same tempo restores the original durations.
        let config = store
            .dispatch("scenes/setTimingUnit", json!({ "unit": "seconds" }))
            .expect("setTimingUnit back");
        assert_eq!(config["timingUnit"], json!("seconds"));
        assert_eq!(config["minSeconds"], json!(8));
        assert_eq!(config["maxSeconds"], json!(24));
        assert_eq!(store.scenes_state().scenes[0]["minSeconds"], json!(1));
        assert_eq!(store.scenes_state().scenes[0]["maxSeconds"], json!(3));

        // At 60 BPM the same seconds are half as many bars.
        store
            .dispatch("beats/SET_BPM", json!({ "bpm": 60 }))
            .expect("bpm");
        let config = store
            .dispatch("scenes/setTimingUnit", json!({ "unit": "bars" }))
            .expect("setTimingUnit at 60 BPM");
        assert_eq!(config["minSeconds"], json!(2));
        assert_eq!(config["maxSeconds"], json!(6));
        assert_eq!(store.scenes_state().scenes[0]["minSeconds"], json!(0.25));
        assert_eq!(store.scenes_state().scenes[0]["maxSeconds"], json!(0.75));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_scene_drops_its_groups_and_list_entry() {
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-remove"));
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-remove" }))
            .expect("apply");
        assert!(!store.state().groups.is_empty());

        store
            .dispatch("scenes/removeScene", json!({ "sceneId": "scene-remove" }))
            .expect("remove");
        assert!(store.scenes_state().scenes.is_empty());
        assert!(store.state().groups.is_empty());
    }

    #[test]
    fn apply_scene_restores_expressions_with_remapped_input_ids() {
        let mut scene = two_row_scene("scene-expressions");
        scene["modules"]["doc-bands"]["$props"] = json!({
            "u_ringWidth": { "id": "doc-input-ring", "type": "float" }
        });
        scene["expressions"] = json!({
            "assignments": {
                "doc-ex-1": {
                    "id": "doc-ex-1",
                    "inputId": "doc-input-ring",
                    "expression": "value * 4",
                    "evaluateEveryFrame": false,
                }
            }
        });

        let mut store = Store::new();
        seed_scene(&mut store, scene);
        store
            .dispatch(
                "scenes/applyScene",
                json!({ "sceneId": "scene-expressions" }),
            )
            .expect("applyScene dispatches");

        let assignment = store
            .state()
            .expressions
            .get("doc-ex-1")
            .expect("expression restored");
        assert_ne!(assignment.input_id, "doc-input-ring", "input id remapped");

        let live = store
            .state()
            .groups
            .iter()
            .find(|group| group.name == "Background")
            .expect("background row");
        let module_id = live.modules[0].clone();
        let module = store.state().modules.get(&module_id).expect("module");
        assert_eq!(
            module.dollar_props["u_ringWidth"]["id"],
            json!(assignment.input_id)
        );

        // A manual write to the Scene's prop goes through the restored
        // expression (`value * 4`).
        store
            .dispatch(
                "modules/updateProp",
                json!({
                    "moduleId": module_id,
                    "prop": "u_ringWidth",
                    "data": { "value": 0.1 },
                }),
            )
            .expect("updateProp dispatches");
        assert!(
            (store.state().modules[&module_id].props["u_ringWidth"]
                .as_f64()
                .unwrap()
                - 0.4)
                .abs()
                < 1e-6
        );

        let types = delta_types(&mut store);
        assert!(types.contains(&"expressions/ADD_EXPRESSION".to_string()));
    }

    #[test]
    fn save_current_as_scene_snapshots_the_live_state() {
        // Scenes.vue's "Save current as Scene" dispatches
        // `scenes/saveCurrentAsScene` and writes the returned document as a
        // Scene file. The engine used to hit the dispatch wildcard and the
        // button silently did nothing.
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-1"));
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-1" }))
            .expect("applyScene dispatches");

        let before = store.scenes_state().scenes.len();
        let saved = store
            .dispatch(
                "scenes/saveCurrentAsScene",
                json!({ "name": "Webcam Halftone" }),
            )
            .expect("saveCurrentAsScene dispatches");

        assert_eq!(saved["name"], json!("Webcam Halftone"));
        assert_eq!(saved["enabled"], json!(true));
        assert_eq!(saved["timingOverride"], json!(false));
        assert!(
            saved["id"].as_str().is_some_and(|id| !id.is_empty()),
            "a Scene id is generated: {saved}"
        );
        assert!(saved["minSeconds"].is_number(), "{saved}");
        assert!(saved["maxSeconds"].is_number(), "{saved}");
        let groups = saved["groups"].as_array().expect("groups");
        assert!(
            groups
                .iter()
                .any(|group| group["name"] == json!("Background")),
            "the live Groups are snapshotted: {groups:?}"
        );
        let modules = saved["modules"].as_object().expect("modules");
        assert!(
            modules
                .values()
                .any(|module| module["$moduleName"] == json!("Bands")),
            "the live Modules are snapshotted: {modules:?}"
        );
        assert_eq!(
            store.scenes_state().scenes.len(),
            before + 1,
            "the Scene list grows by one"
        );

        // An empty name falls back to `Scene <n>` (`scenes.js:1934`).
        let unnamed = store
            .dispatch("scenes/saveCurrentAsScene", json!({}))
            .expect("unnamed save dispatches");
        assert_eq!(unnamed["name"], json!(format!("Scene {}", before + 2)));
    }

    #[test]
    fn scene_list_actions_rename_move_enable_and_retime() {
        // The Scene grid's column actions (`Scenes.vue` / the header context
        // menu) all dispatch engine actions; these used to fall through to the
        // dispatch wildcard and silently do nothing in engine mode.
        let mut store = Store::new();
        for (id, name) in [("scene-a", "Alpha"), ("scene-b", "Beta")] {
            let mut scene = two_row_scene(id);
            scene["name"] = json!(name);
            seed_scene(&mut store, scene);
        }

        // addEmptyScene: a valid empty document with a unique name.
        let empty = store
            .dispatch("scenes/addEmptyScene", json!({ "name": "Empty" }))
            .expect("addEmptyScene dispatches");
        assert_eq!(empty["name"], json!("Empty"));
        assert_eq!(empty["enabled"], json!(true));
        assert_eq!(empty["groups"], json!([]));
        assert!(empty["id"].as_str().is_some_and(|id| !id.is_empty()));
        assert_eq!(store.scenes_state().scenes.len(), 3);

        // updateSceneName: trimmed, case-insensitively unique.
        let renamed = store
            .dispatch(
                "scenes/updateSceneName",
                json!({ "sceneId": "scene-a", "name": "  Alpha Two  " }),
            )
            .expect("updateSceneName dispatches");
        assert_eq!(renamed["name"], json!("Alpha Two"));
        assert_eq!(
            find_default_scene(&store, "Alpha Two")["id"],
            json!("scene-a")
        );
        let duplicate = store
            .dispatch(
                "scenes/updateSceneName",
                json!({ "sceneId": "scene-b", "name": "alpha two" }),
            )
            .expect("duplicate rename dispatches");
        assert_eq!(duplicate, Value::Null, "duplicate names are rejected");

        // setSceneEnabled returns the updated Scene.
        let disabled = store
            .dispatch(
                "scenes/setSceneEnabled",
                json!({ "sceneId": "scene-b", "enabled": false }),
            )
            .expect("setSceneEnabled dispatches");
        assert_eq!(disabled["enabled"], json!(false));
        assert_eq!(find_default_scene(&store, "Beta")["enabled"], json!(false));

        // updateSceneTiming sets an explicit override, reset clears it.
        let timed = store
            .dispatch(
                "scenes/updateSceneTiming",
                json!({ "sceneId": "scene-a", "minSeconds": 4.0, "maxSeconds": 9.0 }),
            )
            .expect("updateSceneTiming dispatches");
        assert_eq!(timed["timingOverride"], json!(true));
        assert_eq!(timed["minSeconds"], json!(4));
        assert_eq!(timed["maxSeconds"], json!(9));

        let reset = store
            .dispatch("scenes/resetSceneTiming", json!({ "sceneId": "scene-a" }))
            .expect("resetSceneTiming dispatches");
        assert_eq!(reset["timingOverride"], json!(false));
        assert_eq!(reset["minSeconds"], json!(8));
        assert_eq!(reset["maxSeconds"], json!(24));

        // moveScene reorders the list (performance order) and returns true.
        assert_eq!(
            store
                .dispatch(
                    "scenes/moveScene",
                    json!({ "sceneId": "scene-b", "toIndex": 0 })
                )
                .expect("moveScene dispatches"),
            json!(true)
        );
        assert_eq!(store.scenes_state().scenes[0]["id"], json!("scene-b"));
        assert_eq!(
            store
                .dispatch(
                    "scenes/moveScene",
                    json!({ "sceneId": "scene-b", "toIndex": 0 })
                )
                .expect("no-op move dispatches"),
            json!(false)
        );
    }

    #[test]
    fn scene_cell_actions_create_capture_and_clear_rows() {
        let mut store = Store::new();
        for (id, name) in [("scene-a", "Alpha"), ("scene-b", "Beta")] {
            let mut scene = two_row_scene(id);
            scene["name"] = json!(name);
            seed_scene(&mut store, scene);
        }
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-a" }))
            .expect("applyScene dispatches");

        // captureSceneRowToScene saves the live row's Group + Module snapshots.
        let captured = store
            .dispatch(
                "scenes/captureSceneRowToScene",
                json!({ "sceneId": "scene-a", "rowIndex": 1 }),
            )
            .expect("captureSceneRowToScene dispatches");
        let row_modules = captured["groups"][1]["modules"]
            .as_array()
            .expect("row 1 modules");
        assert!(
            !row_modules.is_empty(),
            "the live row was captured: {captured}"
        );
        let snapshots = captured["modules"].as_object().expect("modules");
        assert!(
            row_modules
                .iter()
                .filter_map(Value::as_str)
                .any(|id| snapshots.contains_key(id)),
            "the captured Modules have snapshots: {captured}"
        );

        // createSceneCellGroup replaces that live row with a fresh Group.
        let created = store
            .dispatch(
                "scenes/createSceneCellGroup",
                json!({ "sceneId": "scene-b", "rowIndex": 0 }),
            )
            .expect("createSceneCellGroup dispatches");
        let group_id = created["groupId"].as_str().expect("groupId").to_string();
        let live = store
            .state()
            .groups
            .iter()
            .find(|group| group.id == group_id)
            .expect("the row Group exists");
        assert_eq!(live.scene_id.as_deref(), Some("scene-b"));
        assert_eq!(live.scene_row, Some(0));
        assert_eq!(live.name, "Background");

        // clearSceneCell empties the saved row and prunes its snapshots.
        let cleared = store
            .dispatch(
                "scenes/clearSceneCell",
                json!({ "sceneId": "scene-a", "rowIndex": 1 }),
            )
            .expect("clearSceneCell dispatches");
        assert_eq!(cleared["groups"][1]["modules"], json!([]));
    }

    #[test]
    fn clearing_a_scene_cell_empties_the_live_group_and_its_capture() {
        // `clearSceneCell` (`scenes.js:3399-3420`) removes every Module the
        // live row Group owns and then rewrites the Group's module list with
        // `groups/REPLACE_GROUP_MODULES { modules: [] }`. Only the removal had
        // been ported, so the live Group kept phantom module ids and the next
        // cell capture serialised a Group whose ids had no snapshots.
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-a"));
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-a" }))
            .expect("applyScene dispatches");

        let live_row_group = |store: &Store| {
            store
                .state()
                .groups
                .iter()
                .find(|group| {
                    group.scene_id.as_deref() == Some("scene-a")
                        && group.scene_row == Some(1)
                        && group.transition_ghost != Some(true)
                })
                .cloned()
                .expect("the live row-1 Group")
        };
        let live_module_ids = live_row_group(&store).modules;
        assert_eq!(live_module_ids.len(), 1, "the row owns one live Module");

        store
            .dispatch(
                "scenes/clearSceneCell",
                json!({ "sceneId": "scene-a", "rowIndex": 1 }),
            )
            .expect("clearSceneCell dispatches");

        assert!(
            live_row_group(&store).modules.is_empty(),
            "the live Group keeps no phantom module ids: {:?}",
            live_row_group(&store).modules
        );
        for module_id in &live_module_ids {
            assert!(
                !store.state().modules.contains_key(module_id),
                "the removed Module instance is gone: {module_id}"
            );
        }

        // A capture right after the clear round-trips the empty cell instead
        // of writing the phantom ids back into the Scene document.
        let captured = store
            .dispatch(
                "scenes/captureSceneRowToScene",
                json!({ "sceneId": "scene-a", "rowIndex": 1 }),
            )
            .expect("captureSceneRowToScene dispatches");
        assert_eq!(captured["groups"][1]["modules"], json!([]));
        let snapshots = captured["modules"].as_object().expect("module snapshots");
        for module_id in captured["groups"][1]["modules"]
            .as_array()
            .expect("row modules")
            .iter()
            .filter_map(Value::as_str)
        {
            assert!(
                snapshots.contains_key(module_id),
                "every captured module id has a snapshot: {captured}"
            );
        }
        assert!(
            !snapshots.contains_key("doc-ball"),
            "the cleared module snapshot is not resurrected: {captured}"
        );
    }

    #[test]
    fn capturing_an_unowned_row_returns_null_and_leaves_the_scene_alone() {
        // `captureSceneRowToScene` (`scenes.js:3505-3530`) only captures the
        // active row's Group or a Group stamped for this Scene and returns
        // `null` otherwise. The positional `find_live_row_group` fallback wrote
        // whichever Scene owned the live row into this Scene's cell.
        let mut store = Store::new();
        for (id, name) in [("scene-a", "Alpha"), ("scene-b", "Beta")] {
            let mut scene = two_row_scene(id);
            scene["name"] = json!(name);
            seed_scene(&mut store, scene);
        }
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-a" }))
            .expect("applyScene dispatches");

        let beta_before = find_default_scene(&store, "Beta").clone();
        let captured = store
            .dispatch(
                "scenes/captureSceneRowToScene",
                json!({ "sceneId": "scene-b", "rowIndex": 1 }),
            )
            .expect("captureSceneRowToScene dispatches");

        assert_eq!(captured, Value::Null, "no live row belongs to Beta");
        assert_eq!(find_default_scene(&store, "Beta"), &beta_before);
    }

    #[test]
    fn disable_gallery_item_modules_turns_off_enabled_gallery_modules() {
        let mut store = Store::new();
        let gallery = store
            .dispatch("groups/createGroup", json!({ "name": GALLERY_GROUP_NAME }))
            .expect("gallery group");
        let group_id = gallery["id"].as_str().expect("id").to_string();
        let mut module_ids = Vec::new();
        for _ in 0..2 {
            let module = store
                .dispatch(
                    "modules/makeActiveModule",
                    json!({ "moduleName": "Ball", "moduleMeta": { "isGallery": true } }),
                )
                .expect("module");
            let module_id = module["$id"].as_str().expect("module id").to_string();
            store
                .dispatch(
                    "groups/ADD_MODULE_TO_GROUP",
                    json!({ "groupId": group_id, "moduleId": module_id }),
                )
                .expect("add module");
            store
                .dispatch(
                    "modules/UPDATE_ACTIVE_MODULE_META",
                    json!({ "id": module_id, "metaKey": "enabled", "data": true }),
                )
                .expect("enable module");
            module_ids.push(module_id);
        }
        assert!(
            module_ids
                .iter()
                .all(|id| store.state().modules[id].meta["enabled"] == json!(true)),
            "both modules start enabled: {:?}",
            module_ids
                .iter()
                .map(|id| store.state().modules[id].meta.clone())
                .collect::<Vec<_>>()
        );

        store
            .dispatch("groups/disableGalleryItemModules", json!({}))
            .expect("disableGalleryItemModules dispatches");
        assert!(
            module_ids
                .iter()
                .all(|id| store.state().modules[id].meta["enabled"] == json!(false)),
            "every enabled Gallery module is disabled"
        );
    }

    #[test]
    fn gallery_and_group_order_actions_apply_in_the_engine() {
        let mut store = Store::new();
        let first = store
            .dispatch("groups/createGroup", json!({ "name": "First" }))
            .expect("first group");
        let second = store
            .dispatch("groups/createGroup", json!({ "name": "Second" }))
            .expect("second group");
        let first_id = first["id"].as_str().unwrap().to_string();
        let second_id = second["id"].as_str().unwrap().to_string();

        // groups/orderByIds reorders the live layer stack.
        store
            .dispatch("groups/orderByIds", json!({ "ids": [second_id, first_id] }))
            .expect("orderByIds dispatches");
        assert_eq!(store.state().groups[0].id, second_id);
        assert_eq!(store.state().groups[1].id, first_id);

        // groups/focusGalleryItem keeps the focused Gallery module enabled.
        let gallery = store
            .dispatch("groups/createGroup", json!({ "name": GALLERY_GROUP_NAME }))
            .expect("gallery group");
        let gallery_id = gallery["id"].as_str().unwrap().to_string();
        let mut module_ids = Vec::new();
        for _ in 0..3 {
            let module = store
                .dispatch(
                    "modules/makeActiveModule",
                    json!({ "moduleName": "Ball", "moduleMeta": { "isGallery": true } }),
                )
                .expect("module");
            let module_id = module["$id"].as_str().unwrap().to_string();
            store
                .dispatch(
                    "groups/ADD_MODULE_TO_GROUP",
                    json!({ "groupId": gallery_id, "moduleId": module_id }),
                )
                .expect("add module");
            store
                .dispatch(
                    "modules/UPDATE_ACTIVE_MODULE_META",
                    json!({ "id": module_id, "metaKey": "enabled", "data": true }),
                )
                .expect("enable module");
            module_ids.push(module_id);
        }

        store
            .dispatch(
                "groups/focusGalleryItem",
                json!({ "groupId": gallery_id, "moduleId": module_ids[1] }),
            )
            .expect("focusGalleryItem dispatches");
        assert_eq!(
            store.state().modules[&module_ids[1]].meta["enabled"],
            json!(true),
            "the focused module stays enabled"
        );
        for id in [&module_ids[0], &module_ids[2]] {
            assert_eq!(
                store.state().modules[id].meta["enabled"],
                json!(false),
                "the other gallery modules are disabled"
            );
        }
    }

    #[test]
    fn copy_scene_cell_copies_a_row_between_scenes() {
        let mut store = Store::new();
        for (id, name) in [("scene-a", "Alpha"), ("scene-b", "Beta")] {
            let mut scene = two_row_scene(id);
            scene["name"] = json!(name);
            seed_scene(&mut store, scene);
        }
        // Give Alpha's Background a module snapshot the copy can carry.
        let alpha = find_default_scene(&store, "Alpha");
        let source_group = alpha["groups"][0].clone();
        let source_modules = alpha["modules"].as_object().expect("modules").clone();
        let source_module_id = source_modules
            .keys()
            .next()
            .expect("Alpha has a Background module")
            .clone();

        let copied = store
            .dispatch(
                "scenes/copySceneCellToScene",
                json!({
                    "fromSceneId": "scene-a",
                    "toSceneId": "scene-b",
                    "fromRowIndex": 0,
                    "toRowIndex": 0,
                }),
            )
            .expect("copySceneCellToScene dispatches");
        assert_ne!(copied, Value::Null, "the copy succeeded");
        let beta = find_default_scene(&store, "Beta");
        assert_eq!(beta["groups"][0]["name"], source_group["name"]);
        assert!(
            beta["modules"]
                .as_object()
                .expect("modules")
                .contains_key(&source_module_id),
            "the copied module snapshot travels: {}",
            beta["modules"]
        );
        // Beta's own row 1 is untouched.
        assert_eq!(beta["groups"][1]["name"], "Effects");

        // An empty source cell returns null.
        let empty = store
            .dispatch(
                "scenes/copySceneCellToScene",
                json!({
                    "fromSceneId": "scene-a",
                    "toSceneId": "scene-b",
                    "fromRowIndex": 9,
                    "toRowIndex": 0,
                }),
            )
            .expect("empty copy dispatches");
        assert_eq!(empty, Value::Null);
    }

    #[test]
    fn live_scene_rows_are_typed_and_skip_transition_ghosts() {
        let mut store = Store::new();
        seed_scene(&mut store, two_row_scene("scene-live"));
        store
            .dispatch("scenes/applyScene", json!({ "sceneId": "scene-live" }))
            .expect("apply");

        let rows = store.live_scene_rows_by_row();
        assert_eq!(rows.len(), 2, "both Scene rows are live: {rows:?}");
        for row in 0..2 {
            let (scene_id, group_id) = rows.get(&row).expect("row is live");
            assert_eq!(scene_id, "scene-live");
            let group = store
                .state()
                .groups
                .iter()
                .find(|group| &group.id == group_id)
                .expect("the live row names a real Group");
            assert_eq!(group.scene_row, Some(row));
            assert_eq!(group.scene_id.as_deref(), Some("scene-live"));
            assert_eq!(group.transition_ghost, None);
        }

        // A transition ghost is the outgoing half of a row crossfade: it must
        // not claim the row, even though it still carries the Scene stamps.
        let row0_group = rows.get(&0).expect("row 0").1.clone();
        store
            .state
            .groups
            .iter_mut()
            .find(|group| group.id == row0_group)
            .expect("row 0 Group")
            .transition_ghost = Some(true);

        let rows = store.live_scene_rows_by_row();
        assert!(!rows.contains_key(&0), "the ghost row is not live");
        assert_eq!(
            rows.get(&1).map(|(scene_id, _)| scene_id.as_str()),
            Some("scene-live")
        );

        // `scenes/SET_ACTIVE_ROWS` is built from the same typed view.
        let delta = store.live_scene_rows();
        assert!(delta.get("0").is_none());
        assert_eq!(delta["1"]["sceneId"], json!("scene-live"));
        assert_eq!(delta["1"]["groupId"], json!(rows[&1].1));
    }
}
