//! Default Scenes for the browser shell.
//!
//! The desktop app seeds the versioned default Scene data through its `Store`.
//! The browser has no store, so this module ports the pieces the shell needs:
//!
//! * Build a Scene document from a
//!   [`modv_engine::default_scenes::SceneDocument`] (Groups, Modules, input ids,
//!   pinned/automatic audio links and their tweens).
//! * Keep the Scene list, performance row names and disabled cells.
//! * Apply a whole Scene or a single row to the live [`EngineState`].
//! * Capture the live stack back into a Scene.
//!
//! Everything here is pure (no DOM, no wgpu), so it is unit-tested on the host.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use modv_app::module_registry::ModuleRegistry;
use modv_engine::default_scenes::{DEFAULTS_VERSION, SceneDocument, get_scenes_to_seed, scene_key};
use modv_engine::engine_state::{EngineState, GROUP_ENABLED, Group, ModuleInstance};
use modv_engine::props::coerce_prop_value;
use modv_engine::scene_link_defaults::{
    LinkMode, LinkRange, default_link_for_input, default_link_tween, get_bass_link_range,
    get_bpm_link_range, get_feature_smoothing, is_auto_link_excluded,
};
use modv_engine::scene_rows::{
    assign_scene_row_indexes, normalize_disabled_cells, normalize_disabled_cells_for_scenes,
    normalize_row_names,
};
use modv_engine::scene_timing::{
    TimingUnit as EngineTimingUnit, normalize_scene_timing_values, normalize_timing_config,
    seconds_to_unit, unit_to_seconds,
};
use modv_engine::tweens::{TweenDescriptor, TweenSet};
use modv_ui::TimingUnit;
use modv_ui::model::{PerformanceView, SceneCellView, SceneView, normalize_performance_order};

/// The six Group controls that own an input (`groups.js:174-197`).
const GROUP_CONTROL_INPUTS: &[(&str, &str)] = &[
    ("alpha", "alphaInputId"),
    ("enabled", "enabledInputId"),
    ("clearing", "clearingInputId"),
    ("inherit", "inheritInputId"),
    ("compositeOperation", "compositeOperationInputId"),
    ("pipeline", "pipelineInputId"),
];

/// The three Module meta controls that own an input (`modules.js:291-318`).
const MODULE_META_INPUTS: &[&str] = &["alpha", "enabled", "compositeOperation"];

/// Module props preferred for the automatic bass link (`scenes.js:157-178`).
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

/// Module props preferred for the automatic BPM link (`scenes.js:180-194`).
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

/// Monotonic id source for seeded Scene documents and new cells.
struct IdMinter {
    next: u64,
}

impl IdMinter {
    fn new(next: u64) -> Self {
        Self { next }
    }

    fn mint(&mut self, kind: &str) -> String {
        self.next = self.next.saturating_add(1);
        format!("scene-{kind}-{}", self.next)
    }

    fn mint_scene(&mut self) -> String {
        self.next = self.next.saturating_add(1);
        format!("scene-{}", self.next)
    }
}

/// The browser's Scene list plus the performance config that belongs with it.
#[derive(Clone, Debug)]
pub struct SceneLibrary {
    scenes: Vec<Value>,
    performance: Value,
    defaults_version: i64,
    seeded: Vec<String>,
    seeded_ids: Vec<String>,
    next_id: u64,
}

impl Default for SceneLibrary {
    fn default() -> Self {
        Self::empty()
    }
}

impl SceneLibrary {
    /// An empty library at the current defaults version.
    pub fn empty() -> Self {
        Self {
            scenes: Vec::new(),
            performance: default_performance(),
            defaults_version: 0,
            seeded: Vec::new(),
            seeded_ids: Vec::new(),
            next_id: 0,
        }
    }

    /// A fresh library with every default Scene seeded.
    pub fn seed(registry: &ModuleRegistry) -> Self {
        let mut library = Self::empty();
        library.seed_missing(registry);
        library
    }

    /// Add any defaults introduced since the persisted defaults version.
    pub fn seed_missing(&mut self, registry: &ModuleRegistry) {
        let existing_names: Vec<String> = self
            .scenes
            .iter()
            .filter_map(|scene| scene.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        let definitions = get_scenes_to_seed(self.defaults_version, &self.seeded, &existing_names);
        let mut ids = IdMinter::new(self.next_id);
        for definition in definitions {
            let (document, _) =
                build_default_scene_document(registry, definition, &mut ids, &json!({}));
            let name = definition.name.clone();
            let id = document
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            self.scenes.push(document);
            self.seeded.push(name);
            self.seeded_ids.push(id);
        }
        self.next_id = ids.next;
        self.defaults_version = DEFAULTS_VERSION;
    }

    /// Scene documents in grid order.
    pub fn scenes(&self) -> &[Value] {
        &self.scenes
    }

    /// The performance config.
    pub fn performance(&self) -> &Value {
        &self.performance
    }

    /// The next document id the library will mint.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// A plain JSON document for local storage.
    pub fn to_json(&self) -> Value {
        json!({
            "scenes": self.scenes.clone(),
            "performance": self.performance.clone(),
            "defaultsVersion": self.defaults_version,
            "seededDefaultScenes": self.seeded.clone(),
            "seededDefaultSceneIds": self.seeded_ids.clone(),
            "nextId": self.next_id,
        })
    }

    /// Restore a persisted library. Returns `None` for an invalid document.
    pub fn from_json(value: &Value) -> Option<Self> {
        let scenes = value.get("scenes")?.as_array()?.clone();
        let performance = value
            .get("performance")
            .cloned()
            .unwrap_or_else(default_performance);
        let defaults_version = value
            .get("defaultsVersion")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let seeded = string_list(value.get("seededDefaultScenes"));
        let seeded_ids = string_list(value.get("seededDefaultSceneIds"));
        let next_id = value.get("nextId").and_then(Value::as_u64).unwrap_or(0);
        Some(Self {
            scenes,
            performance,
            defaults_version,
            seeded,
            seeded_ids,
            next_id,
        })
    }

    /// Row names from the performance config.
    pub fn row_names(&self) -> Vec<String> {
        normalize_row_names(self.performance.get("rowNames"))
    }

    /// Number of grid rows: named rows plus the deepest Scene stack.
    pub fn row_count(&self) -> usize {
        let named = self.row_names().len();
        let deepest = self
            .scenes
            .iter()
            .map(|scene| {
                scene
                    .get("groups")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
            })
            .max()
            .unwrap_or(0);
        named.max(deepest)
    }

    /// Performance-disabled cells, pruned against the current Scene list.
    pub fn disabled_cells(&self) -> std::collections::BTreeMap<String, Vec<usize>> {
        normalize_disabled_cells_for_scenes(
            self.performance.get("disabledCells"),
            &self.scenes,
            &self.row_names(),
        )
    }

    /// The runtime Scene id shown by the grid (never persisted as state).
    pub fn current_scene_id(&self) -> Option<String> {
        self.performance
            .get("currentSceneId")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// Set the runtime Scene id shown by the grid.
    pub fn set_current_scene(&mut self, scene_id: Option<&str>) {
        set_performance_value(
            &mut self.performance,
            "currentSceneId",
            scene_id.map_or(Value::Null, |id| json!(id)),
        );
    }

    /// The read model the Scenes grid draws.
    pub fn performance_view(&self) -> PerformanceView {
        performance_view(&self.performance)
    }

    /// The Scene grid cells.
    pub fn views(&self) -> Vec<SceneView> {
        let row_names = self.row_names();
        let disabled = normalize_disabled_cells_for_scenes(
            self.performance.get("disabledCells"),
            &self.scenes,
            &row_names,
        );
        let performance = self.performance_view();
        let timing = (performance.min_value, performance.max_value);
        self.scenes
            .iter()
            .enumerate()
            .map(|(index, scene)| scene_view(scene, index, &row_names, &disabled, timing))
            .collect()
    }

    /// Find a Scene document by id.
    pub fn find(&self, scene_id: &str) -> Option<&Value> {
        self.scenes
            .iter()
            .find(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id))
    }

    fn find_mut(&mut self, scene_id: &str) -> Option<&mut Value> {
        self.scenes
            .iter_mut()
            .find(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id))
    }

    /// Apply a whole Scene, replacing the live layer stack.
    pub fn apply(
        &self,
        scene_id: &str,
        registry: &ModuleRegistry,
        state: &mut EngineState,
        tweens: &mut TweenSet,
        bpm: f64,
    ) -> Option<String> {
        let scene = self.find(scene_id)?;
        apply_scene(scene, registry, state, tweens, bpm)
    }

    /// Apply one Scene row, leaving the other live rows alone.
    pub fn apply_row(
        &self,
        scene_id: &str,
        row: usize,
        registry: &ModuleRegistry,
        state: &mut EngineState,
        tweens: &mut TweenSet,
        bpm: f64,
    ) -> bool {
        let Some(scene) = self.find(scene_id).cloned() else {
            return false;
        };
        let Some(group_value) = scene
            .get("groups")
            .and_then(Value::as_array)
            .and_then(|groups| groups.get(row))
        else {
            return false;
        };
        let Some(materialized) = materialize_group(group_value, &scene, row, registry) else {
            return false;
        };
        merge_scene_runtime(&scene, state, tweens, bpm);
        remove_live_row(state, scene_id, row);
        let index = row.min(state.groups.len());
        for id in &materialized.group.modules {
            if let Some(module) = materialized.modules.get(id) {
                state.modules.insert(id.clone(), module.clone());
            }
        }
        state.groups.insert(index, materialized.group);
        true
    }

    /// `scenes/addEmptyScene`.
    pub fn add_empty(&mut self, name: Option<&str>) -> String {
        let id = self.mint_scene();
        let name = match name.map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) if !self.name_taken(name, None) => name.to_string(),
            _ => self.unique_name(),
        };
        let min = self
            .performance
            .get("minSeconds")
            .and_then(Value::as_f64)
            .unwrap_or(8.0);
        let max = self
            .performance
            .get("maxSeconds")
            .and_then(Value::as_f64)
            .unwrap_or(24.0);
        let document = empty_scene_document(&id, &name, min, max);
        self.scenes.push(document);
        id
    }

    /// `scenes/updateSceneName`: trimmed and unique (case-insensitive).
    pub fn rename(&mut self, scene_id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.name_taken(name, Some(scene_id)) {
            return false;
        }
        let Some(scene) = self.find_mut(scene_id) else {
            return false;
        };
        if let Some(object) = scene.as_object_mut() {
            object.insert("name".to_string(), json!(name));
        }
        true
    }

    /// `scenes/removeScene`.
    pub fn remove(&mut self, scene_id: &str) -> bool {
        let before = self.scenes.len();
        self.scenes
            .retain(|scene| scene.get("id").and_then(Value::as_str) != Some(scene_id));
        self.seeded.retain(|name| {
            self.scenes
                .iter()
                .any(|scene| scene.get("name").and_then(Value::as_str) == Some(name.as_str()))
        });
        if let Some(disabled) = self
            .performance
            .get_mut("disabledCells")
            .and_then(Value::as_object_mut)
        {
            disabled.remove(scene_id);
        }
        self.scenes.len() != before
    }

    /// `scenes/moveScene`.
    pub fn move_to(&mut self, scene_id: &str, to_index: i64) -> bool {
        let Some(from) = self
            .scenes
            .iter()
            .position(|scene| scene.get("id").and_then(Value::as_str) == Some(scene_id))
        else {
            return false;
        };
        let to = to_index.clamp(0, self.scenes.len().saturating_sub(1) as i64) as usize;
        if from == to {
            return false;
        }
        let scene = self.scenes.remove(from);
        self.scenes.insert(to, scene);
        true
    }

    /// `scenes/setSceneEnabled`.
    pub fn set_enabled(&mut self, scene_id: &str, enabled: bool) -> bool {
        let Some(scene) = self.find_mut(scene_id) else {
            return false;
        };
        if let Some(object) = scene.as_object_mut() {
            object.insert("enabled".to_string(), json!(enabled));
        }
        true
    }

    /// `scenes/updateSceneTiming`.
    pub fn set_timing(&mut self, scene_id: &str, min: f64, max: f64) -> bool {
        let Some(scene) = self.find_mut(scene_id) else {
            return false;
        };
        if let Some(object) = scene.as_object_mut() {
            object.insert("timingOverride".to_string(), json!(true));
            object.insert("minSeconds".to_string(), json_number(min));
            object.insert("maxSeconds".to_string(), json_number(max));
        }
        true
    }

    /// `scenes/resetSceneTiming`.
    pub fn reset_timing(&mut self, scene_id: &str) -> bool {
        let Some(scene) = self.find_mut(scene_id) else {
            return false;
        };
        if let Some(object) = scene.as_object_mut() {
            object.insert("timingOverride".to_string(), json!(false));
            object.remove("minSeconds");
            object.remove("maxSeconds");
        }
        true
    }

    /// `scenes/setCellEnabled`.
    pub fn set_cell_enabled(&mut self, scene_id: &str, row: usize, enabled: bool) -> bool {
        if self.find(scene_id).is_none() {
            return false;
        }
        let mut disabled = normalize_disabled_cells(self.performance.get("disabledCells"));
        let rows = disabled.entry(scene_id.to_string()).or_default();
        if enabled {
            rows.retain(|index| *index != row);
        } else if !rows.contains(&row) {
            rows.push(row);
            rows.sort_unstable();
        }
        if rows.is_empty() {
            disabled.remove(scene_id);
        }
        let filtered = disabled
            .into_iter()
            .map(|(id, rows)| {
                (
                    id,
                    Value::Array(rows.into_iter().map(|row| json!(row)).collect()),
                )
            })
            .collect::<Map<String, Value>>();
        set_performance_value(
            &mut self.performance,
            "disabledCells",
            Value::Object(filtered),
        );
        true
    }

    /// `scenes/clearSceneCell`.
    pub fn clear_cell(&mut self, scene_id: &str, row: usize) -> bool {
        let Some(scene) = self.find_mut(scene_id) else {
            return false;
        };
        let Some(object) = scene.as_object_mut() else {
            return false;
        };
        let Some(groups) = object.get_mut("groups").and_then(Value::as_array_mut) else {
            return false;
        };
        if row >= groups.len() {
            return false;
        }
        let removed = groups[row].clone();
        groups[row] = Value::Null;
        let Some(group) = removed.as_object() else {
            return true;
        };
        let removed_ids: Vec<String> = group
            .get("modules")
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if removed_ids.is_empty() {
            return true;
        }
        let still_used: HashSet<String> = groups
            .iter()
            .filter_map(|group| group.get("modules").and_then(Value::as_array))
            .flat_map(|ids| ids.iter().filter_map(Value::as_str).map(str::to_string))
            .collect();
        if let Some(modules) = object.get_mut("modules").and_then(Value::as_object_mut) {
            for id in removed_ids {
                if !still_used.contains(&id) {
                    modules.remove(&id);
                }
            }
        }
        true
    }

    /// `scenes/copySceneCellToScene`: clone one row's Group and Modules.
    pub fn copy_cell(
        &mut self,
        from_scene_id: &str,
        from_row: usize,
        to_scene_id: &str,
        to_row: usize,
    ) -> bool {
        if from_scene_id == to_scene_id && from_row == to_row {
            return false;
        }
        let Some(source) = self.find(from_scene_id).cloned() else {
            return false;
        };
        let Some(group) = source
            .get("groups")
            .and_then(Value::as_array)
            .and_then(|groups| groups.get(from_row))
            .cloned()
        else {
            return false;
        };
        let copied_modules = module_values_for_group(&source, &group);
        let ids = referenced_input_ids(&group, &copied_modules);
        let Some(target) = self.find_mut(to_scene_id) else {
            return false;
        };
        let Some(object) = target.as_object_mut() else {
            return false;
        };
        if let Some(groups) = object.get_mut("groups").and_then(Value::as_array_mut) {
            while groups.len() <= to_row {
                groups.push(Value::Null);
            }
            groups[to_row] = group;
        }
        if let Some(modules) = object.get_mut("modules").and_then(Value::as_object_mut) {
            for (id, module) in copied_modules {
                modules.insert(id, module);
            }
        }
        merge_scene_io(object, &source, &ids);
        true
    }

    /// `scenes/createSceneCellGroup`: materialise an empty Group at a row.
    pub fn create_cell_group(&mut self, scene_id: &str, row: usize) -> bool {
        let row_names = self.row_names();
        let name = row_names
            .get(row)
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Row {}", row + 1));
        let group_id = self.mint("group");
        let mut group = new_group_value(&group_id, &name, GROUP_ENABLED);
        // Group control inputs are installed lazily when the cell is applied;
        // seed them now so the Scene document is self-contained.
        let mut inputs = Map::new();
        for (control, key) in GROUP_CONTROL_INPUTS {
            let input_id = self.mint("input");
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
        let Some(scene) = self.find_mut(scene_id) else {
            return false;
        };
        let Some(object) = scene.as_object_mut() else {
            return false;
        };
        if let Some(groups) = object.get_mut("groups").and_then(Value::as_array_mut) {
            while groups.len() <= row {
                groups.push(Value::Null);
            }
            groups[row] = Value::Object(group);
        }
        if let Some(scene_inputs) = object
            .get_mut("inputs")
            .and_then(|inputs| inputs.get_mut("inputs"))
            .and_then(Value::as_object_mut)
        {
            scene_inputs.extend(inputs);
        }
        true
    }

    /// `scenes/addModuleToCell`: append a registered Module to a Scene row.
    pub fn add_module_to_cell(
        &mut self,
        registry: &ModuleRegistry,
        scene_id: &str,
        row: usize,
        module_name: &str,
    ) -> bool {
        if self.find(scene_id).is_none() {
            return false;
        }
        let mut ids = IdMinter::new(self.next_id);
        let Some((module_id, module, mut inputs)) =
            build_module_document(registry, module_name, &mut ids)
        else {
            self.next_id = ids.next;
            return false;
        };
        if self
            .find(scene_id)
            .and_then(|scene| scene.get("groups"))
            .and_then(Value::as_array)
            .and_then(|groups| groups.get(row))
            .is_none_or(Value::is_null)
        {
            self.next_id = ids.next;
            if !self.create_cell_group(scene_id, row) {
                return false;
            }
        }
        let Some(object) = self.find_mut(scene_id).and_then(Value::as_object_mut) else {
            self.next_id = ids.next;
            return false;
        };
        if let Some(group) = object
            .get_mut("groups")
            .and_then(Value::as_array_mut)
            .and_then(|groups| groups.get_mut(row))
            .and_then(Value::as_object_mut)
            && let Some(modules) = group.get_mut("modules").and_then(Value::as_array_mut)
        {
            modules.push(json!(module_id));
        }
        if let Some(modules) = object.get_mut("modules").and_then(Value::as_object_mut) {
            modules.insert(module_id.clone(), module);
        }
        if let Some(scene_inputs) = object
            .get_mut("inputs")
            .and_then(|inputs| inputs.get_mut("inputs"))
            .and_then(Value::as_object_mut)
        {
            scene_inputs.append(&mut inputs);
        }
        self.next_id = ids.next;
        true
    }

    /// `scenes/setSceneCellGroup`: snapshot a live Group into a Scene row.
    pub fn set_cell_group(
        &mut self,
        scene_id: &str,
        row: usize,
        state: &EngineState,
        group_id: &str,
        tweens: &TweenSet,
    ) -> bool {
        let Some(live) = state.find_group(group_id) else {
            return false;
        };
        let Some(scene) = self.find_mut(scene_id) else {
            return false;
        };
        let (group, modules) = capture_group(live, state);
        let runtime_group = group.clone();
        let runtime_modules = modules.clone();
        let Some(object) = scene.as_object_mut() else {
            return false;
        };
        if let Some(groups) = object.get_mut("groups").and_then(Value::as_array_mut) {
            while groups.len() <= row {
                groups.push(Value::Null);
            }
            groups[row] = group;
        }
        if let Some(map) = object.get_mut("modules").and_then(Value::as_object_mut) {
            map.extend(modules);
        }
        merge_captured_runtime(object, &runtime_group, &runtime_modules, state);
        merge_captured_tweens(object, tweens);
        true
    }

    /// `scenes/captureLiveStateToScene`.
    pub fn capture_live_state(
        &mut self,
        scene_id: &str,
        state: &EngineState,
        tweens: &TweenSet,
    ) -> bool {
        let Some(scene) = self.find(scene_id) else {
            return false;
        };
        let scene_id = scene
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let scene_has_groups = scene
            .get("groups")
            .and_then(Value::as_array)
            .is_some_and(|groups| !groups.is_empty());
        let groups: Vec<&Group> = state
            .groups
            .iter()
            .filter(|group| {
                group.name != "Gallery"
                    && group.transition_ghost != Some(true)
                    && (!scene_has_groups || group.scene_id.as_deref() == Some(scene_id.as_str()))
            })
            .collect();
        if groups.is_empty() {
            return false;
        }
        let mut group_values = Vec::new();
        let mut module_values = Map::new();
        for group in groups {
            let (group_value, modules) = capture_group(group, state);
            group_values.push(group_value);
            module_values.extend(modules);
        }
        let Some(object) = self.find_mut(&scene_id).and_then(Value::as_object_mut) else {
            return false;
        };
        object.insert("groups".to_string(), Value::Array(group_values));
        object.insert("modules".to_string(), Value::Object(module_values));
        object.insert(
            "inputs".to_string(),
            json!({
                "focusedInput": state.focused_input.clone(),
                "inputs": state.inputs,
                "inputLinks": state.input_links,
            }),
        );
        object.insert(
            "expressions".to_string(),
            json!({ "assignments": state.expressions }),
        );
        merge_captured_tweens(object, tweens);
        true
    }

    /// `scenes/captureSceneRowToScene`.
    pub fn capture_live_row(
        &mut self,
        scene_id: &str,
        row: usize,
        state: &EngineState,
        tweens: &TweenSet,
    ) -> bool {
        let live = state
            .groups
            .iter()
            .find(|group| {
                group.scene_id.as_deref() == Some(scene_id) && group.scene_row == Some(row)
            })
            .or_else(|| state.groups.get(row));
        let Some(live) = live else {
            return false;
        };
        self.set_cell_group(scene_id, row, state, &live.id, tweens)
    }

    /// `scenes/updatePerformance`: merge a config update.
    pub fn update_performance(&mut self, config: &Map<String, Value>) -> bool {
        if let Some(object) = self.performance.as_object_mut() {
            for (key, value) in config {
                object.insert(key.clone(), value.clone());
            }
        } else {
            self.performance = Value::Object(config.clone());
        }
        true
    }

    /// `scenes/setRowNames`.
    pub fn set_row_names(&mut self, names: &Value) -> bool {
        let normalized = normalize_row_names(Some(names));
        set_performance_value(
            &mut self.performance,
            "rowNames",
            Value::Array(normalized.into_iter().map(Value::String).collect()),
        );
        true
    }

    /// `scenes/setPerformanceEnabled`.
    pub fn set_performance_enabled(&mut self, enabled: bool) -> bool {
        set_performance_value(&mut self.performance, "enabled", json!(enabled));
        true
    }

    /// `scenes/setTimingUnit`: convert the global and per-Scene overrides
    /// through seconds at the current tempo.
    pub fn set_timing_unit(&mut self, unit: &str, bpm: f64) -> bool {
        let requested = EngineTimingUnit::parse(Some(&json!(unit)));
        let old = self.performance.clone();
        let current = normalize_timing_config(Some(&old));
        let min_seconds = unit_to_seconds(current.min, current.unit, bpm);
        let max_seconds = unit_to_seconds(current.max, current.unit, bpm);
        let min = seconds_to_unit(min_seconds, requested, bpm);
        let max = seconds_to_unit(max_seconds, requested, bpm);
        for scene in &mut self.scenes {
            let values = normalize_scene_timing_values(Some(scene), Some(&old));
            if !values.timing_override {
                continue;
            }
            let scene_min = unit_to_seconds(values.min, values.unit, bpm);
            let scene_max = unit_to_seconds(values.max, values.unit, bpm);
            if let Some(object) = scene.as_object_mut() {
                object.insert(
                    "minSeconds".to_string(),
                    json_number(seconds_to_unit(scene_min, requested, bpm)),
                );
                object.insert(
                    "maxSeconds".to_string(),
                    json_number(seconds_to_unit(scene_max, requested, bpm)),
                );
            }
        }
        set_performance_value(
            &mut self.performance,
            "timingUnit",
            json!(requested.as_str()),
        );
        set_performance_value(&mut self.performance, "minSeconds", json_number(min));
        set_performance_value(&mut self.performance, "maxSeconds", json_number(max));
        true
    }

    fn mint(&mut self, kind: &str) -> String {
        self.next_id = self.next_id.saturating_add(1);
        format!("scene-{kind}-{}", self.next_id)
    }

    fn mint_scene(&mut self) -> String {
        self.next_id = self.next_id.saturating_add(1);
        format!("scene-{}", self.next_id)
    }

    fn name_taken(&self, name: &str, except: Option<&str>) -> bool {
        let key = scene_key(name);
        self.scenes.iter().any(|scene| {
            scene.get("id").and_then(Value::as_str) != except
                && scene
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|existing| scene_key(existing) == key)
        })
    }

    fn unique_name(&self) -> String {
        let mut index = self.scenes.len() + 1;
        loop {
            let name = format!("Scene {index}");
            if !self.name_taken(&name, None) {
                return name;
            }
            index += 1;
        }
    }
}

/// Build one default Scene document from its versioned definition.
fn build_default_scene_document(
    registry: &ModuleRegistry,
    definition: &SceneDocument,
    ids: &mut IdMinter,
    midi: &Value,
) -> (Value, Vec<String>) {
    let mut groups: Vec<Value> = Vec::new();
    let mut modules: Map<String, Value> = Map::new();
    let mut inputs: Map<String, Value> = Map::new();
    let mut input_links: Map<String, Value> = Map::new();
    let mut tweens: Vec<Value> = Vec::new();

    for group_definition in &definition.groups {
        let group_id = ids.mint("group");
        let mut group = new_group_value(&group_id, &group_definition.name, GROUP_ENABLED);
        if let Some(alpha) = group_definition.alpha {
            group.insert("alpha".to_string(), json_number(alpha));
        }
        if let Some(inherit) = group_definition.inherit {
            group.insert("inherit".to_string(), json!(inherit));
        }
        if let Some(clearing) = group_definition.clearing {
            group.insert("clearing".to_string(), json!(clearing));
        }
        if let Some(operation) = &group_definition.composite_operation {
            group.insert("compositeOperation".to_string(), json!(operation));
        }
        for (control, key) in GROUP_CONTROL_INPUTS {
            let input_id = ids.mint("input");
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
            let Some((module_id, module, mut module_inputs)) =
                build_module_document(registry, &module_definition.name, ids)
            else {
                continue;
            };
            let Some(module_object) = module.as_object() else {
                continue;
            };
            let mut module_object = module_object.clone();
            if let Some(meta) = module_object.get_mut("meta").and_then(Value::as_object_mut)
                && let Some(overrides) = &module_definition.meta
            {
                for (key, value) in overrides {
                    meta.insert(key.clone(), value.clone());
                }
            }
            if let Some(props) = module_object
                .get_mut("props")
                .and_then(Value::as_object_mut)
                && let Some(overrides) = &module_definition.props
            {
                for (prop, value) in overrides {
                    let coerced = registry
                        .prop_schema(&module_definition.name, prop)
                        .map(|schema| coerce_prop_value(&schema, value))
                        .unwrap_or_else(|| value.clone());
                    props.insert(prop.clone(), coerced);
                }
            }
            group_module_ids.push(json!(module_id));
            modules.insert(module_id, Value::Object(module_object));
            inputs.append(&mut module_inputs);
        }
        group.insert("modules".to_string(), Value::Array(group_module_ids));
        groups.push(Value::Object(group));
    }

    let mut smoothing_ids: Vec<String> = Vec::new();
    let mut build = SceneBuild {
        groups: &groups,
        modules: &modules,
        input_links: &mut input_links,
        tweens: &mut tweens,
        smoothing_ids: &mut smoothing_ids,
    };
    apply_default_scene_input_links(definition, &mut build);

    let id = ids.mint_scene();
    let mut document = Map::new();
    document.insert("id".to_string(), json!(id));
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
    (Value::Object(document), smoothing_ids)
}

/// Build one Module document plus its input descriptors.
fn build_module_document(
    registry: &ModuleRegistry,
    module_name: &str,
    ids: &mut IdMinter,
) -> Option<(String, Value, Map<String, Value>)> {
    let mut meta = registry.meta_fields(module_name)?;
    meta.insert("alpha".to_string(), json!(1));
    meta.insert("enabled".to_string(), json!(true));
    meta.insert("compositeOperation".to_string(), json!("normal"));
    meta.insert("isGallery".to_string(), json!(false));

    let module_id = ids.mint("module");
    let mut props = registry
        .default_props(module_name)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    // Keep the key order stable for serialised comparisons.
    props.sort_keys();

    let mut dollar_props = Map::new();
    let mut inputs = Map::new();
    for prop in props.keys() {
        let input_id = ids.mint("input");
        inputs.insert(
            input_id.clone(),
            json!({
                "id": input_id,
                "type": "action",
                "data": { "moduleId": module_id, "prop": prop },
                "getLocation": format!("modules.active[\"{module_id}\"].props[\"{prop}\"]"),
                "location": "modules/updateProp",
            }),
        );
        let mut descriptor = Map::new();
        descriptor.insert("id".to_string(), json!(input_id));
        if let Some(schema) = registry.prop_schema(module_name, prop) {
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
        let input_id = ids.mint("input");
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
        "$moduleName": module_name,
        "meta": Value::Object(meta),
        "props": Value::Object(props),
        "data": {},
        "$props": Value::Object(dollar_props),
    });
    Some((module_id, module, inputs))
}

fn new_group_value(id: &str, name: &str, enabled: i64) -> Map<String, Value> {
    let mut group = Map::new();
    group.insert("id".to_string(), json!(id));
    group.insert("name".to_string(), json!(name));
    group.insert("hidden".to_string(), json!(false));
    group.insert("modules".to_string(), json!([]));
    group.insert("enabled".to_string(), json!(enabled));
    group.insert("alpha".to_string(), json_number(1.0));
    group.insert("inherit".to_string(), json!(true));
    group.insert("inheritFrom".to_string(), json!(-1));
    group.insert("pipeline".to_string(), json!(false));
    group.insert("clearing".to_string(), json!(true));
    group.insert("compositeOperation".to_string(), json!("normal"));
    group
}

fn empty_scene_document(id: &str, name: &str, min: f64, max: f64) -> Value {
    json!({
        "id": id,
        "name": name,
        "enabled": true,
        "minSeconds": json_number(min),
        "maxSeconds": json_number(max),
        "timingOverride": false,
        "groups": [],
        "modules": {},
        "inputs": {
            "focusedInput": { "id": Value::Null, "title": Value::Null },
            "inputs": {},
            "inputLinks": {},
        },
        "expressions": { "assignments": {} },
        "tweens": [],
        "midi": {},
    })
}

// ————————————————— Scene application —————————————————

struct MaterializedGroup {
    group: Group,
    modules: HashMap<String, ModuleInstance>,
}

fn apply_scene(
    scene: &Value,
    registry: &ModuleRegistry,
    state: &mut EngineState,
    tweens: &mut TweenSet,
    bpm: f64,
) -> Option<String> {
    let scene_id = scene.get("id").and_then(Value::as_str)?.to_string();
    let groups = scene.get("groups").and_then(Value::as_array)?;
    let mut live_groups = Vec::new();
    let mut modules = HashMap::new();
    for (row, group_value) in groups.iter().enumerate() {
        let Some(materialized) = materialize_group(group_value, scene, row, registry) else {
            continue;
        };
        for (id, module) in materialized.modules {
            modules.insert(id, module);
        }
        live_groups.push(materialized.group);
    }
    if live_groups.is_empty() {
        return None;
    }

    // A whole-Scene apply replaces the live stack, so stale tween runtimes
    // from the previous Scene must go too.
    *tweens = TweenSet::new();
    merge_scene_runtime(scene, state, tweens, bpm);
    // Hand-built Groups that belong to no Scene survive a Scene apply, like
    // the native replacement rule.
    let retained: Vec<Group> = state
        .groups
        .iter()
        .filter(|group| {
            group.scene_id.is_none()
                && !group.modules.is_empty()
                && group.transition_ghost != Some(true)
        })
        .cloned()
        .collect();
    let mut retained_modules = HashMap::new();
    for group in &retained {
        for module_id in &group.modules {
            if let Some(module) = state.modules.get(module_id) {
                retained_modules.insert(module_id.clone(), module.clone());
            }
        }
    }
    let first = live_groups.first()?.id.clone();
    state.modules.clear();
    for (id, module) in retained_modules {
        state.modules.insert(id, module);
    }
    for (id, module) in modules {
        state.modules.insert(id, module);
    }
    state.groups = retained;
    state.groups.extend(live_groups);
    // Keep the Scene stamp in sync even when the document omitted it.
    for group in &mut state.groups {
        if group.scene_id.is_some() {
            group.scene_id = Some(scene_id.clone());
        }
    }
    Some(first)
}

fn materialize_group(
    group_value: &Value,
    scene: &Value,
    row: usize,
    registry: &ModuleRegistry,
) -> Option<MaterializedGroup> {
    if group_value.is_null() {
        return None;
    }
    let scene_id = scene.get("id").and_then(Value::as_str).unwrap_or_default();
    let module_documents = scene.get("modules").and_then(Value::as_object);
    let mut group: Group = serde_json::from_value(group_value.clone()).ok()?;
    let module_ids = group.modules.clone();
    group.modules.clear();
    let mut modules = HashMap::new();
    for id in module_ids {
        let Some(module_value) = module_documents.and_then(|modules| modules.get(&id)) else {
            continue;
        };
        let Some(module_name) = module_value.get("$moduleName").and_then(Value::as_str) else {
            continue;
        };
        if registry.meta_fields(module_name).is_none() {
            continue;
        }
        let Ok(module) = serde_json::from_value::<ModuleInstance>(module_value.clone()) else {
            continue;
        };
        group.modules.push(id.clone());
        modules.insert(id, module);
    }
    group.scene_id = Some(scene_id.to_string());
    group.scene_row = Some(row);
    Some(MaterializedGroup { group, modules })
}

fn remove_live_row(state: &mut EngineState, scene_id: &str, row: usize) {
    let removed: Vec<String> = state
        .groups
        .iter()
        .filter(|group| group.scene_id.as_deref() == Some(scene_id) && group.scene_row == Some(row))
        .flat_map(|group| group.modules.clone())
        .collect();
    state.groups.retain(|group| {
        !(group.scene_id.as_deref() == Some(scene_id) && group.scene_row == Some(row))
    });
    for id in removed {
        state.modules.shift_remove(&id);
    }
}

/// Merge a Scene's inputs, links and tweens into the live state.
fn merge_scene_runtime(scene: &Value, state: &mut EngineState, tweens: &mut TweenSet, bpm: f64) {
    if let Some(inputs) = scene
        .pointer("/inputs/inputs")
        .and_then(Value::as_object)
        .cloned()
    {
        for (id, input) in inputs {
            if let Ok(input) = serde_json::from_value(input) {
                state.inputs.insert(id, input);
            }
        }
    }
    if let Some(links) = scene
        .pointer("/inputs/inputLinks")
        .and_then(Value::as_object)
        .cloned()
    {
        for (id, link) in links {
            if let Ok(link) = serde_json::from_value(link) {
                state.input_links.insert(id, link);
            }
        }
    }
    if let Some(focused) = scene.pointer("/inputs/focusedInput").cloned() {
        state.focused_input = focused;
    }
    if let Some(assignments) = scene
        .pointer("/expressions/assignments")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
    {
        state.expressions = assignments;
    }
    if let Some(values) = scene.get("tweens").and_then(Value::as_array).cloned() {
        state.tweens = Value::Array(values.clone());
        for value in values {
            if let Ok(descriptor) = serde_json::from_value::<TweenDescriptor>(value) {
                tweens.insert_descriptor(descriptor, 60.0, bpm);
            }
        }
    }
}

fn capture_group(group: &Group, state: &EngineState) -> (Value, Map<String, Value>) {
    let mut group_value = serde_json::to_value(group).unwrap_or(Value::Null);
    let mut modules = Map::new();
    if let Some(object) = group_value.as_object_mut() {
        let ids: Vec<Value> = group
            .modules
            .iter()
            .filter(|id| state.modules.contains_key(*id))
            .map(|id| json!(id))
            .collect();
        object.insert("modules".to_string(), Value::Array(ids));
    }
    for id in &group.modules {
        if let Some(module) = state.modules.get(id)
            && let Ok(value) = serde_json::to_value(module)
        {
            modules.insert(id.clone(), value);
        }
    }
    (group_value, modules)
}

/// Every input id a Group/Module document references.
fn referenced_input_ids(group: &Value, modules: &Map<String, Value>) -> HashSet<String> {
    let mut ids = HashSet::new();
    for (_, key) in GROUP_CONTROL_INPUTS {
        if let Some(id) = group.get(*key).and_then(Value::as_str) {
            ids.insert(id.to_string());
        }
    }
    for module in modules.values() {
        if let Some(meta) = module.get("meta").and_then(Value::as_object) {
            for key in MODULE_META_INPUTS {
                if let Some(id) = meta
                    .get(format!("{key}InputId").as_str())
                    .and_then(Value::as_str)
                {
                    ids.insert(id.to_string());
                }
            }
        }
        if let Some(props) = module.get("$props").and_then(Value::as_object) {
            for descriptor in props.values() {
                if let Some(id) = descriptor.get("id").and_then(Value::as_str) {
                    ids.insert(id.to_string());
                }
            }
        }
    }
    ids
}

/// Copy the live input and link definitions a captured Group/Module set
/// references into the Scene document, so applying the cell later restores
/// its Group controls and prop links.
fn merge_captured_runtime(
    document: &mut Map<String, Value>,
    group: &Value,
    modules: &Map<String, Value>,
    state: &EngineState,
) {
    let ids = referenced_input_ids(group, modules);
    if !document.get("inputs").is_some_and(Value::is_object) {
        document.insert(
            "inputs".to_string(),
            json!({
                "focusedInput": { "id": Value::Null, "title": Value::Null },
                "inputs": {},
                "inputLinks": {},
            }),
        );
    }
    let Some(inputs) = document.get_mut("inputs").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(scene_inputs) = inputs.get_mut("inputs").and_then(Value::as_object_mut) {
        for id in &ids {
            if let Some(input) = state.inputs.get(id)
                && let Ok(value) = serde_json::to_value(input)
            {
                scene_inputs.insert(id.clone(), value);
            }
        }
    }
    if let Some(links) = inputs.get_mut("inputLinks").and_then(Value::as_object_mut) {
        for id in &ids {
            if let Some(link) = state.input_links.get(id)
                && let Ok(value) = serde_json::to_value(link)
            {
                links.insert(id.clone(), value);
            }
        }
    }
}

/// Copy inputs/links/tweens for `ids` from another Scene document.
fn merge_scene_io(document: &mut Map<String, Value>, source: &Value, ids: &HashSet<String>) {
    if !document.get("inputs").is_some_and(Value::is_object) {
        document.insert(
            "inputs".to_string(),
            json!({
                "focusedInput": { "id": Value::Null, "title": Value::Null },
                "inputs": {},
                "inputLinks": {},
            }),
        );
    }
    let source_inputs = source.pointer("/inputs/inputs").and_then(Value::as_object);
    let source_links = source
        .pointer("/inputs/inputLinks")
        .and_then(Value::as_object);
    let Some(inputs) = document.get_mut("inputs").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(scene_inputs) = inputs.get_mut("inputs").and_then(Value::as_object_mut) {
        for id in ids {
            if let Some(input) = source_inputs.and_then(|inputs| inputs.get(id)) {
                scene_inputs.insert(id.clone(), input.clone());
            }
        }
    }
    if let Some(links) = inputs.get_mut("inputLinks").and_then(Value::as_object_mut) {
        for id in ids {
            if let Some(link) = source_links.and_then(|links| links.get(id)) {
                links.insert(id.clone(), link.clone());
            }
        }
    }
    let source_tweens = source.get("tweens").and_then(Value::as_array);
    if let Some(source_tweens) = source_tweens {
        let target_tweens = document
            .entry("tweens".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(target_tweens) = target_tweens.as_array_mut() {
            for tween in source_tweens {
                let Some(id) = tween.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if !ids.contains(id) {
                    continue;
                }
                let exists = target_tweens
                    .iter()
                    .any(|existing| existing.get("id").and_then(Value::as_str) == Some(id));
                if !exists {
                    target_tweens.push(tween.clone());
                }
            }
        }
    }
}

fn merge_captured_tweens(document: &mut Map<String, Value>, tweens: &TweenSet) {
    let values: Vec<Value> = tweens
        .iter()
        .filter_map(|(_, runtime)| serde_json::to_value(runtime.descriptor()).ok())
        .collect();
    document.insert("tweens".to_string(), Value::Array(values));
}

// ————————————————— Read models —————————————————

fn performance_view(config: &Value) -> PerformanceView {
    let number =
        |key: &str, fallback: f64| config.get(key).and_then(Value::as_f64).unwrap_or(fallback);
    let timing_unit = TimingUnit::parse(
        config
            .get("timingUnit")
            .and_then(Value::as_str)
            .unwrap_or("seconds"),
    );
    let (default_min, default_max) = modv_ui::default_timing_values(timing_unit);
    PerformanceView {
        enabled: config
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        current_scene_id: config
            .get("currentSceneId")
            .and_then(Value::as_str)
            .map(str::to_string),
        row_names: normalize_row_names(config.get("rowNames")),
        timing_unit,
        min_value: number("minSeconds", default_min),
        max_value: number("maxSeconds", default_max),
        transition_seconds: number("transitionSeconds", 2.0),
        allowed_transitions: config
            .get("allowedTransitions")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        order: normalize_performance_order(
            config
                .get("order")
                .and_then(Value::as_str)
                .unwrap_or("in-order"),
        )
        .to_string(),
        available_transitions: Vec::new(),
    }
}

fn scene_view(
    scene: &Value,
    index: usize,
    row_names: &[String],
    disabled_cells: &std::collections::BTreeMap<String, Vec<usize>>,
    timing_fallback: (f64, f64),
) -> SceneView {
    let id = scene
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("scene-{index}"));
    let name = scene
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("Scene {}", index + 1));
    let enabled = scene
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let timing = if scene
        .get("timingOverride")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let min = scene
            .get("minSeconds")
            .and_then(Value::as_f64)
            .unwrap_or(timing_fallback.0);
        let max = scene
            .get("maxSeconds")
            .and_then(Value::as_f64)
            .unwrap_or(timing_fallback.1);
        Some((min, max))
    } else {
        None
    };
    let module_names = scene.get("modules").and_then(Value::as_object);
    let groups = scene.get("groups").and_then(Value::as_array);
    let row_indexes = groups.map(|groups| assign_scene_row_indexes(groups, row_names));
    let mut cells: Vec<SceneCellView> = if row_names.iter().any(|name| !name.is_empty()) {
        vec![SceneCellView::default(); row_names.len()]
    } else {
        Vec::new()
    };
    if let Some(groups) = groups {
        for (document_row, group) in groups.iter().enumerate() {
            if group.is_null() {
                continue;
            }
            let row = row_indexes
                .as_ref()
                .and_then(|indexes| indexes.get(document_row))
                .copied()
                .flatten()
                .unwrap_or(document_row);
            let names = group
                .get("modules")
                .and_then(Value::as_array)
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| {
                            let id = id.as_str()?;
                            module_names?
                                .get(id)?
                                .get("$moduleName")?
                                .as_str()
                                .map(str::to_string)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            while cells.len() <= row {
                cells.push(SceneCellView::default());
            }
            cells[row] = SceneCellView {
                row,
                module_names: names,
                disabled: disabled_cells
                    .get(&id)
                    .is_some_and(|rows| rows.contains(&row)),
            };
        }
    }
    SceneView {
        id,
        name,
        enabled,
        timing,
        cells,
    }
}

fn module_values_for_group(scene: &Value, group: &Value) -> Map<String, Value> {
    let mut values = Map::new();
    let Some(modules) = scene.get("modules").and_then(Value::as_object) else {
        return values;
    };
    let Some(ids) = group.get("modules").and_then(Value::as_array) else {
        return values;
    };
    for id in ids {
        let Some(id) = id.as_str() else {
            continue;
        };
        if let Some(module) = modules.get(id) {
            values.insert(id.to_string(), module.clone());
        }
    }
    values
}

fn set_performance_value(performance: &mut Value, key: &str, value: Value) {
    if !performance.is_object() {
        *performance = default_performance();
    }
    if let Some(object) = performance.as_object_mut() {
        object.insert(key.to_string(), value);
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn json_number(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or_else(|| json!(0))
}

// ————————————————— Default Scene links —————————————————

/// The mutable pieces of a Scene snapshot under construction.
struct SceneBuild<'a> {
    groups: &'a [Value],
    modules: &'a Map<String, Value>,
    input_links: &'a mut Map<String, Value>,
    tweens: &'a mut Vec<Value>,
    /// Smoothing ids minted for explicit bass links.
    smoothing_ids: &'a mut Vec<String>,
}

/// A resolved link target inside the Scene snapshot under construction.
struct LinkTarget {
    input_id: String,
    module: Option<Value>,
    prop_name: String,
}

fn apply_default_scene_input_links(definition: &SceneDocument, build: &mut SceneBuild<'_>) {
    let mut excluded: HashSet<String> = HashSet::new();

    for link in &definition.input_links {
        let Some(target) = resolve_scene_link_target(build, link) else {
            continue;
        };
        let range = LinkRange {
            min: link.min.unwrap_or(0.2),
            max: link.max.unwrap_or(1.0),
        };
        if matches!(link.source.as_deref(), Some("tween" | "draw")) {
            create_bpm_tween_link(
                build,
                &target,
                range,
                link.source.as_deref().unwrap_or("tween"),
                link.bpm_division.map(|value| json!(value)),
                link.tween.as_ref(),
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
            .unwrap_or(LinkRange {
                min: 0.55,
                max: 1.0,
            });
        create_bass_link(build, &target, "bass", range, None);
        excluded.insert(target.input_id);
    } else if let Some(target) = find_group_alpha_target(build, LINK_GROUP_PRIORITY, &excluded) {
        create_bass_link(
            build,
            &target,
            "bass",
            LinkRange {
                min: 0.55,
                max: 1.0,
            },
            None,
        );
        excluded.insert(target.input_id);
    }

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
            .unwrap_or(LinkRange { min: 0.4, max: 1.0 });
        create_bpm_tween_link(build, &target, range, "tween", None, None);
    } else if let Some(target) = find_group_alpha_target(build, BPM_GROUP_PRIORITY, &excluded) {
        create_bpm_tween_link(
            build,
            &target,
            LinkRange { min: 0.4, max: 1.0 },
            "tween",
            None,
            None,
        );
    }
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
        });
    };

    let module = find_module_in_group(build, group, module_name)?;
    let input_id = module_control_input_id(&module, control)?;
    Some(LinkTarget {
        input_id,
        module: Some(module),
        prop_name: control.to_string(),
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
            });
        }
    }
    None
}

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
            });
        }
    }
    None
}

fn create_bass_link(
    build: &mut SceneBuild<'_>,
    target: &LinkTarget,
    feature: &str,
    range: LinkRange,
    smoothing: Option<f64>,
) {
    let link = default_link_for_input(LinkMode::Bass, &target.input_id, Some(feature), Some(range));
    let feature = link.feature.clone().unwrap_or_else(|| "bass".to_string());

    let mut args = vec![json!(feature)];
    if let Some(amount) = smoothing.and_then(|amount| get_feature_smoothing(Some(&json!(amount)))) {
        let smoothing_id = format!("scene-smoothing-{}", build.smoothing_ids.len() + 1);
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
    range: LinkRange,
    source: &str,
    bpm_division: Option<Value>,
    tween: Option<&Value>,
) {
    let link = default_link_for_input(LinkMode::Bpm, &target.input_id, None, Some(range));
    let descriptor = match tween {
        Some(payload) => match payload.as_object() {
            Some(object) => {
                let mut descriptor = object.clone();
                descriptor.insert("id".to_string(), json!(target.input_id));
                Value::Object(descriptor)
            }
            None => default_link_tween(&target.input_id, bpm_division.as_ref()),
        },
        None => default_link_tween(&target.input_id, bpm_division.as_ref()),
    };
    build.tweens.push(descriptor);
    let value = json!({
        "id": target.input_id,
        "type": link.r#type,
        "location": link.location,
        "source": source,
        "min": json_number(link.range.min),
        "max": json_number(link.range.max),
    });
    build.input_links.insert(target.input_id.clone(), value);
}
