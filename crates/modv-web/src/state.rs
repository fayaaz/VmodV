//! Browser-owned engine state.
//!
//! The desktop shell's `Store` (presets, Scenes, filesystem, NDI, OSC, MIDI,
//! plugins) is deliberately not part of the web build. This module owns the
//! subset the browser shell needs: live Groups, their active modules, prop
//! editing, and the `EngineSnapshot` the existing egui shell renders.
//!
//! The state is pure (no wgpu, no DOM), so it is unit-tested on the host.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::sync::{Arc, OnceLock};

use include_dir::{Dir, include_dir};
use serde_json::{Map, Value, json};

use modv_app::compositor::RowFadeInfo;
use modv_app::input_links::{BeatsState, LinkTarget, LinkWrite, evaluate_input_links};
use modv_app::module_registry::ModuleRegistry;
use modv_app::scene_transition::{SceneTransitionInfo, TransitionEffect};
use modv_engine::engine_state::{
    EngineState, GROUP_DISABLED, GROUP_ENABLED, Group, InheritFrom, ModuleInstance,
};
use modv_engine::inputs::{Input, InputLink};
use modv_engine::props::coerce_prop_value;
use modv_engine::scene_rows::{
    PickNextRowScene, ShouldAdvanceRow, pick_next_row_scene_with_disabled, should_advance_row,
};
use modv_engine::scene_timing::{
    normalize_transition_duration, pick_cell_dwell_numeric, resolve_scene_timing_at,
};
use modv_engine::scene_transition_pick::{
    FADE_TRANSITION, NATIVE_TRANSITION_NAMES, NONE_TRANSITION, pick_transition_name,
};
use modv_engine::scenes_playlist::normalize_transition_list;
use modv_engine::tweens::{TweenDescriptor, TweenSet};
use modv_renderers::two_d::AudioFrame;
use modv_ui::model::{
    AudioView, EngineHealth, EngineSnapshot, GroupView, InheritTarget, InputLinkView,
    ModuleMetaView, ModuleView, PropEnumMember, PropView, TweenView,
};
use modv_ui::state::UiAction;

use crate::scenes::SceneLibrary;

/// The browser's live engine document.
pub struct WebState {
    registry: Arc<ModuleRegistry>,
    state: EngineState,
    group_id: String,
    next_id: u64,
    audio: AudioFrame,
    error: Option<String>,
    frames: u64,
    adapter: String,
    backend: String,
    bpm: f64,
    tweens: TweenSet,
    smoothing: HashMap<String, f64>,
    beats: BeatsState,
    scenes: SceneLibrary,
    performance_runtime: PerformanceRuntime,
    transition: Option<WholeTransition>,
    row_transitions: BTreeMap<usize, RowTransition>,
    pending_transition_capture: Option<TransitionEffect>,
    dirty: bool,
}

/// One performance row's dwell state (the native `rowRuntime`).
#[derive(Clone, Debug, Default)]
struct PerformanceRow {
    scene_id: Option<String>,
    dwell: f64,
    target: f64,
}

/// Browser port of the native performance runtime's per-row dwell loop.
#[derive(Clone, Debug, Default)]
struct PerformanceRuntime {
    rows: Vec<PerformanceRow>,
    active: BTreeMap<usize, String>,
    rng: u64,
}

impl PerformanceRuntime {
    /// xorshift64* — the same shape as the native runtime's RNG.
    fn next_random(&mut self) -> f64 {
        let mut state = self.rng;
        if state == 0 {
            state = 0x9E37_79B9_7F4A_7C15;
        }
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.rng = state;
        ((state.wrapping_mul(0x2545_F491_4F6C_DD1D)) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// An in-flight whole-Scene transition (the native `transition` slot).
#[derive(Clone, Debug)]
struct WholeTransition {
    from: Vec<(String, f64)>,
    to: Vec<(String, f64)>,
    effect: Option<TransitionEffect>,
    elapsed: f64,
    duration: f64,
}

impl WholeTransition {
    fn progress(&self) -> f64 {
        if self.duration.is_finite() && self.duration > 0.0 {
            (self.elapsed / self.duration).min(1.0)
        } else {
            1.0
        }
    }
}

/// An in-flight per-row crossfade (`rowTransitions`).
#[derive(Clone, Debug)]
struct RowTransition {
    row: usize,
    group_id: String,
    ghost_group_id: String,
    from_alpha: f64,
    target_alpha: f64,
    elapsed: f64,
    duration: f64,
}

impl RowTransition {
    fn progress(&self) -> f64 {
        if self.duration.is_finite() && self.duration > 0.0 {
            (self.elapsed / self.duration).min(1.0)
        } else {
            1.0
        }
    }
}

fn smoothstep(progress: f64) -> f64 {
    let t = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    t * t * (3.0 - 2.0 * t)
}

impl Default for WebState {
    fn default() -> Self {
        Self::new()
    }
}

impl WebState {
    /// A fresh state with the built-in shader/2D/Point10 registry plus the
    /// bundled ISF corpus.
    pub fn new() -> Self {
        let mut state = EngineState::new();
        let group_id = "web-group-1".to_string();
        state
            .groups
            .push(new_group(&group_id, "Group 1", GROUP_ENABLED));
        let registry = bundled_registry();
        let scenes = SceneLibrary::seed(&registry);
        let mut this = Self {
            registry,
            state,
            group_id,
            next_id: 0,
            audio: AudioFrame::default(),
            error: None,
            frames: 0,
            adapter: "Browser adapter".to_string(),
            backend: "webgpu".to_string(),
            bpm: 0.0,
            tweens: TweenSet::new(),
            smoothing: HashMap::new(),
            beats: BeatsState::default(),
            scenes,
            performance_runtime: PerformanceRuntime::default(),
            transition: None,
            row_transitions: BTreeMap::new(),
            pending_transition_capture: None,
            dirty: false,
        };
        this.install_group_inputs("web-group-1");
        this
    }

    /// The seeded Scene library (defaults plus edits).
    pub fn scene_library(&self) -> &SceneLibrary {
        &self.scenes
    }

    /// Whether live state or Scenes changed since the last browser save.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clear the dirty flag after a successful save.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Serialise the live stack, Scenes and tweens for local storage.
    pub fn to_persisted_json(&self) -> Value {
        let tweens: Vec<Value> = self
            .tweens
            .iter()
            .filter_map(|(_, runtime)| serde_json::to_value(runtime.descriptor()).ok())
            .collect();
        json!({
            "version": 1,
            "engine": serde_json::to_value(&self.state).unwrap_or(Value::Null),
            "tweens": tweens,
            "scenes": self.scenes.to_json(),
            "groupId": self.group_id,
            "nextId": self.next_id,
        })
    }

    /// Restore a saved document. Invalid or incompatible documents leave the
    /// current (fresh) state untouched.
    pub fn restore_persisted_json(&mut self, value: &Value) -> bool {
        if value.get("version").and_then(Value::as_u64) != Some(1) {
            return false;
        }
        let Some(engine) = value.get("engine") else {
            return false;
        };
        let Ok(mut state) = serde_json::from_value::<EngineState>(engine.clone()) else {
            return false;
        };
        let Some(mut scenes) = value.get("scenes").and_then(SceneLibrary::from_json) else {
            return false;
        };
        scenes.seed_missing(&self.registry);

        // Drop instances whose renderer is no longer registered, then remove
        // their dangling Group references.
        let known: HashSet<String> = state
            .modules
            .iter()
            .filter(|(_, module)| self.registry.meta_fields(&module.module_name).is_some())
            .map(|(id, _)| id.clone())
            .collect();
        state.modules.retain(|id, _| known.contains(id));
        for group in &mut state.groups {
            group.modules.retain(|id| state.modules.contains_key(id));
        }

        let mut tweens = TweenSet::new();
        if let Some(values) = value.get("tweens").and_then(Value::as_array) {
            for value in values {
                if let Ok(descriptor) = serde_json::from_value::<TweenDescriptor>(value.clone()) {
                    tweens.insert_descriptor(descriptor, 60.0, self.bpm);
                }
            }
            state.tweens = Value::Array(values.clone());
        }

        let group_id = value
            .get("groupId")
            .and_then(Value::as_str)
            .filter(|id| state.find_group(id).is_some())
            .map(str::to_string)
            .or_else(|| state.groups.first().map(|group| group.id.clone()))
            .unwrap_or_default();
        let next_id = value.get("nextId").and_then(Value::as_u64).unwrap_or(0);

        self.state = state;
        self.scenes = scenes;
        self.tweens = tweens;
        self.group_id = group_id;
        self.next_id = next_id;
        self.smoothing.clear();
        self.dirty = false;
        if self.performance_enabled() {
            self.start_performance();
        }
        true
    }

    /// The authoritative engine state handed to the compositor.
    pub fn state(&self) -> &EngineState {
        &self.state
    }

    /// The module registry shared with the compositor.
    pub fn registry(&self) -> &ModuleRegistry {
        &self.registry
    }

    /// A shared handle to the registry (the compositor's constructor takes one).
    pub fn registry_arc(&self) -> Arc<ModuleRegistry> {
        Arc::clone(&self.registry)
    }

    /// The default Group's id.
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    /// The current audio frame, updated from WebAudio once per animation frame.
    pub fn audio(&self) -> &AudioFrame {
        &self.audio
    }

    /// The current tempo estimate (0 when no confident BPM yet).
    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    /// Replace the audio frame.
    pub fn set_audio(&mut self, audio: AudioFrame) {
        self.audio = audio;
    }

    /// Evaluate every audio/draw/tween Input Link against the latest frame.
    ///
    /// This mirrors the native Store's per-frame link pass; the browser state
    /// is the authoritative store, so writes land directly on the module
    /// props, module meta and group controls the compositor reads next.
    pub fn evaluate_links(&mut self) {
        self.beats.bpm = self.bpm;
        let mut writes = Vec::new();
        evaluate_input_links(
            &self.state,
            &self.audio,
            &self.beats,
            &mut self.tweens,
            &mut self.smoothing,
            &mut writes,
        );
        for write in writes {
            self.apply_link_write(&write);
        }
    }

    fn apply_link_write(&mut self, write: &LinkWrite) {
        match &write.target {
            LinkTarget::Module {
                module_id,
                prop,
                path,
            } => {
                if path.is_some() {
                    return;
                }
                let Some(module) = self.state.modules.get(module_id) else {
                    return;
                };
                let schema = self.registry.prop_schema(&module.module_name, prop);
                let value = schema
                    .as_ref()
                    .map(|schema| coerce_prop_value(schema, &json!(write.value)))
                    .unwrap_or_else(|| json!(write.value));
                let Some(module) = self.state.modules.get_mut(module_id) else {
                    return;
                };
                if let Some(props) = module.props.as_object_mut() {
                    props.insert(prop.clone(), value);
                }
            }
            LinkTarget::ModuleMeta {
                module_id,
                meta_key,
            } => {
                let value = match meta_key.as_str() {
                    "enabled" => json!(write.value != 0.0),
                    "alpha" => json!(write.value),
                    "compositeOperation" => return,
                    _ => return,
                };
                let Some(module) = self.state.modules.get_mut(module_id) else {
                    return;
                };
                if let Some(meta) = module.meta.as_object_mut() {
                    meta.insert(meta_key.clone(), value);
                }
            }
            LinkTarget::Group { group_id, key } => {
                let Some(group) = self.state.find_group_mut(group_id) else {
                    return;
                };
                match key.as_str() {
                    "alpha" => group.alpha = write.value,
                    "enabled" => group.enabled = write.value as i64,
                    "hidden" => group.hidden = write.value != 0.0,
                    _ => {}
                }
            }
        }
    }

    fn upsert_input_link(&mut self, payload: &Value) -> bool {
        let Some(input_id) = payload.get("inputId").and_then(Value::as_str) else {
            return false;
        };
        let Some(mut object) = payload.as_object().cloned() else {
            return false;
        };
        object.insert("id".to_string(), json!(input_id));
        let Ok(link) = serde_json::from_value::<InputLink>(Value::Object(object)) else {
            return false;
        };
        self.state.input_links.insert(input_id.to_string(), link);
        true
    }

    fn update_input_link(&mut self, payload: &Value) -> bool {
        let Some(input_id) = payload.get("inputId").and_then(Value::as_str) else {
            return false;
        };
        let Some(key) = payload.get("key").and_then(Value::as_str) else {
            return false;
        };
        let Some(value) = payload.get("value") else {
            return false;
        };
        let Some(link) = self.state.input_links.get_mut(input_id) else {
            return false;
        };
        match key {
            "args" => link.args = serde_json::from_value(value.clone()).ok(),
            "location" => link.location = value.as_str().map(str::to_string),
            "source" => link.source = value.as_str().map(str::to_string),
            "min" => link.min = value.as_f64(),
            "max" => link.max = value.as_f64(),
            "match" => link.matcher = Some(value.clone()),
            "type" => {
                let Ok(kind) = serde_json::from_value(value.clone()) else {
                    return false;
                };
                link.kind = kind;
            }
            _ => return false,
        }
        true
    }

    fn create_tween(&mut self, payload: &Value) -> bool {
        let payload = payload.get("tween").unwrap_or(payload);
        let id = payload
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| self.next_input_id());
        let data = payload
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| vec![json!([0.0]), json!([1.0])]);
        let mut descriptor = TweenDescriptor::new(&id, value_to_rows(&data));
        if let Some(duration) = payload.get("duration").and_then(Value::as_f64) {
            descriptor.duration = duration;
        }
        if let Some(direction) = payload.get("direction").and_then(Value::as_str) {
            descriptor.direction = direction.to_string();
        }
        if let Some(easing) = payload.get("easing").and_then(Value::as_str) {
            descriptor.easing = easing.to_string();
        }
        if let Some(looping) = payload.get("loop").and_then(Value::as_bool) {
            descriptor.looping = looping;
        }
        if let Some(loop_mode) = payload.get("loopMode").and_then(Value::as_str) {
            descriptor.loop_mode = Some(loop_mode.to_string());
        }
        if let Some(use_bpm) = payload.get("useBpm").and_then(Value::as_bool) {
            descriptor.use_bpm = use_bpm;
        }
        if let Some(division) = payload.get("bpmDivision").and_then(Value::as_f64) {
            descriptor.bpm_division = division;
        }
        if let Some(total) = payload.get("durationAsTotalTime").and_then(Value::as_bool) {
            descriptor.duration_as_total_time = total;
        }
        if let Some(steps) = payload.get("steps").and_then(Value::as_f64) {
            descriptor.steps = steps;
        }
        if let Some(times) = payload.get("keyframeTimes") {
            descriptor.keyframe_times = serde_json::from_value(times.clone()).ok();
        }
        if let Some(is_gallery) = payload.get("isGallery").and_then(Value::as_bool) {
            descriptor.is_gallery = Some(is_gallery);
        }
        let value = descriptor.to_json();
        push_tween_json(&mut self.state.tweens, value);
        self.tweens.insert_descriptor(descriptor, 60.0, self.bpm);
        true
    }

    fn remove_tween(&mut self, id: &str) -> bool {
        self.tweens.remove(id);
        remove_tween_json(&mut self.state.tweens, id)
    }

    /// Count a presented frame (the status bar's frame counter).
    pub fn note_frame(&mut self) {
        self.frames = self.frames.saturating_add(1);
    }

    /// The last shell-level error shown in the status bar.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Set or clear the status-bar error.
    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    /// Report the adapter/back end eframe actually selected (`WebGPU` or the
    /// WebGL2 fallback) in the status bar.
    pub fn set_backend(&mut self, adapter: impl Into<String>, backend: impl Into<String>) {
        self.adapter = adapter.into();
        self.backend = backend.into();
    }

    /// Set the tempo shown by the meters (the demo audio runs at this BPM).
    pub fn set_bpm(&mut self, bpm: f64) {
        if bpm.is_finite() && bpm > 0.0 {
            self.bpm = bpm;
            self.beats.bpm = bpm;
        }
    }

    /// Apply the analyser's tempo/beat result for this frame.
    pub fn set_tempo(&mut self, bpm: Option<f32>, kick: bool) {
        self.bpm = bpm
            .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
            .map(f64::from)
            .unwrap_or(0.0);
        self.beats.bpm = self.bpm;
        self.beats.kick = kick;
    }

    fn next_input_id(&mut self) -> String {
        self.next_id = self.next_id.saturating_add(1);
        format!("web-input-{}", self.next_id)
    }

    /// Give a Group the control inputs its Inspector link rows read.
    fn install_group_inputs(&mut self, group_id: &str) {
        let mut alpha = None;
        let mut enabled = None;
        for (key, slot) in [("alpha", &mut alpha), ("enabled", &mut enabled)] {
            let input_id = self.next_input_id();
            self.state.inputs.insert(
                input_id.clone(),
                Input {
                    id: input_id.clone(),
                    type_: "commit".to_string(),
                    data: json!({ "groupId": group_id, "key": key }),
                    get_location: None,
                    location: Some("groups/UPDATE_GROUP_BY_KEY".to_string()),
                },
            );
            *slot = Some(input_id);
        }
        if let Some(group) = self.state.find_group_mut(group_id) {
            group.alpha_input_id = alpha;
            group.enabled_input_id = enabled;
        }
    }

    /// Create the prop and meta Input records behind a Module's link rows.
    fn install_module_inputs(&mut self, module_id: &str, module_name: &str) {
        let Some(module) = self.state.modules.get(module_id) else {
            return;
        };
        let Some(props) = module.props.as_object() else {
            return;
        };
        let prop_values: Vec<(String, Value)> = props
            .iter()
            .map(|(prop, value)| (prop.clone(), value.clone()))
            .collect();
        let mut descriptors = Map::new();
        for (prop, value) in prop_values {
            let input_id = self.next_input_id();
            let schema = self.registry.prop_schema(module_name, &prop);
            let mut descriptor = Map::new();
            descriptor.insert("id".to_string(), json!(input_id));
            if let Some(schema) = &schema {
                descriptor.insert("type".to_string(), json!(schema.type_));
                if let Some(min) = schema.min {
                    descriptor.insert("min".to_string(), json!(min));
                }
                if let Some(max) = schema.max {
                    descriptor.insert("max".to_string(), json!(max));
                }
            }
            descriptor.insert("default".to_string(), value);
            descriptors.insert(prop.clone(), Value::Object(descriptor));
            self.state.inputs.insert(
                input_id.clone(),
                Input {
                    id: input_id,
                    type_: "action".to_string(),
                    data: json!({ "moduleId": module_id, "prop": prop }),
                    get_location: Some(format!(
                        "modules.active[\"{module_id}\"].props[\"{prop}\"]"
                    )),
                    location: Some("modules/updateProp".to_string()),
                },
            );
        }

        let mut meta_ids = Vec::new();
        for (key, field) in [
            ("alpha", "alphaInputId"),
            ("enabled", "enabledInputId"),
            ("compositeOperation", "compositeOperationInputId"),
        ] {
            let input_id = self.next_input_id();
            self.state.inputs.insert(
                input_id.clone(),
                Input {
                    id: input_id.clone(),
                    type_: "action".to_string(),
                    data: json!({ "moduleId": module_id, "metaKey": key }),
                    get_location: None,
                    location: Some("modules/updateMeta".to_string()),
                },
            );
            meta_ids.push((field, input_id));
        }

        if let Some(module) = self.state.modules.get_mut(module_id) {
            module.dollar_props = Value::Object(descriptors);
            if let Some(meta) = module.meta.as_object_mut() {
                for (field, input_id) in meta_ids {
                    meta.insert(field.to_string(), json!(input_id));
                }
            }
        }
    }

    /// Add a registered module to the default Group and return its new id.
    pub fn add_module(&mut self, module_name: &str) -> Option<String> {
        self.add_module_to_group(module_name, &self.group_id.clone())
    }

    /// Add a registered module to `group_id` and return its new id.
    pub fn add_module_to_group(&mut self, module_name: &str, group_id: &str) -> Option<String> {
        let Some(mut meta) = self.registry.meta_fields(module_name) else {
            self.error = Some(format!("module {module_name:?} is not registered"));
            return None;
        };
        let props = self
            .registry
            .default_props(module_name)
            .unwrap_or_else(|| json!({}));

        // The Store leaves new modules disabled; the browser's drop gesture
        // means "use this now", so enable it immediately. Gallery previews are
        // not part of the web shell.
        meta.insert("alpha".to_string(), json!(1));
        meta.insert("enabled".to_string(), json!(true));
        meta.insert("compositeOperation".to_string(), json!("normal"));
        meta.insert("isGallery".to_string(), json!(false));

        self.next_id = self.next_id.saturating_add(1);
        let id = format!("web-module-{}", self.next_id);
        let module = ModuleInstance {
            id: id.clone(),
            module_name: module_name.to_string(),
            meta: Value::Object(meta),
            props,
            data: json!({}),
            dollar_props: json!({}),
            status: Vec::new(),
            extra: Map::new(),
        };
        self.state.modules.insert(id.clone(), module);
        self.install_module_inputs(&id, module_name);

        let target = if self.state.find_group(group_id).is_some() {
            group_id.to_string()
        } else {
            self.group_id.clone()
        };
        if let Some(group) = self.state.find_group_mut(&target) {
            group.modules.push(id.clone());
        }
        self.error = None;
        self.dirty = true;
        Some(id)
    }

    /// Remove an active module from every Group, with its links and tweens.
    pub fn remove_module(&mut self, module_id: &str) -> bool {
        let removed = self.state.modules.shift_remove(module_id).is_some();
        if !removed {
            return false;
        }
        self.dirty = true;
        let input_ids: Vec<String> = self
            .state
            .inputs
            .iter()
            .filter(|(_, input)| {
                input
                    .data
                    .get("moduleId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == module_id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for input_id in input_ids {
            self.state.inputs.shift_remove(&input_id);
            self.state.input_links.shift_remove(&input_id);
            self.tweens.remove(&input_id);
            remove_tween_json(&mut self.state.tweens, &input_id);
        }
        for group in &mut self.state.groups {
            group.modules.retain(|id| id != module_id);
        }
        true
    }

    /// Apply one prop edit, coerced through the registry schema.
    pub fn update_prop(&mut self, module_id: &str, prop: &str, value: Value) -> bool {
        let Some(module) = self.state.modules.get_mut(module_id) else {
            return false;
        };
        let Some(props) = module.props.as_object_mut() else {
            return false;
        };
        if !props.contains_key(prop) {
            return false;
        }
        let schema = self.registry.prop_schema(&module.module_name, prop);
        let value = schema
            .as_ref()
            .map(|schema| coerce_prop_value(schema, &value))
            .unwrap_or(value);
        props.insert(prop.to_string(), value);
        true
    }

    /// Apply one meta edit (`enabled`, `alpha`, `compositeOperation`).
    pub fn update_meta(&mut self, module_id: &str, key: &str, value: Value) -> bool {
        if !matches!(key, "enabled" | "alpha" | "compositeOperation") {
            return false;
        }
        let Some(module) = self.state.modules.get_mut(module_id) else {
            return false;
        };
        let Some(meta) = module.meta.as_object_mut() else {
            return false;
        };
        meta.insert(key.to_string(), value);
        true
    }

    /// Dispatch one UI action. Returns whether the shell understood it.
    /// Start performance mode: give every row its first Scene and dwell.
    fn start_performance(&mut self) {
        let row_count = self.scenes.row_count();
        let row_names = self.scenes.row_names();
        let disabled = self.scenes.disabled_cells();
        let order = self
            .scenes
            .performance()
            .get("order")
            .and_then(Value::as_str)
            .unwrap_or("in-order")
            .to_string();
        let mut runtime = PerformanceRuntime::default();
        for row in 0..row_count {
            let picked = {
                let args = PickNextRowScene {
                    scenes: self.scenes.scenes(),
                    row_index: row,
                    current_scene_id: None,
                    order: &order,
                    row_names: &row_names,
                };
                pick_next_row_scene_with_disabled(&args, &disabled, || runtime.next_random())
                    .cloned()
            };
            let Some(scene) = picked else {
                runtime.rows.push(PerformanceRow::default());
                continue;
            };
            let Some(scene_id) = scene.get("id").and_then(Value::as_str).map(str::to_string) else {
                runtime.rows.push(PerformanceRow::default());
                continue;
            };
            if !self.scenes.apply_row(
                &scene_id,
                row,
                &self.registry,
                &mut self.state,
                &mut self.tweens,
                self.bpm,
            ) {
                runtime.rows.push(PerformanceRow::default());
                continue;
            }
            let target = {
                let timing = resolve_scene_timing_at(
                    Some(&scene),
                    Some(self.scenes.performance()),
                    self.bpm,
                );
                let roll = runtime.next_random();
                pick_cell_dwell_numeric(
                    Some(timing.min_seconds),
                    Some(timing.max_seconds),
                    Some(roll),
                )
            };
            runtime.rows.push(PerformanceRow {
                scene_id: Some(scene_id.clone()),
                dwell: 0.0,
                target,
            });
            runtime.active.insert(row, scene_id);
        }
        self.performance_runtime = runtime;
        if let Some(scene_id) = self.performance_runtime.active.values().next().cloned() {
            self.scenes.set_current_scene(Some(&scene_id));
        }
        self.dirty = true;
    }

    fn stop_performance(&mut self) {
        self.finish_whole_transition(true);
        self.finish_all_row_transitions();
        self.performance_runtime = PerformanceRuntime::default();
    }

    fn performance_enabled(&self) -> bool {
        self.scenes
            .performance()
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Advance every row whose dwell deadline has elapsed.
    pub fn tick_performance(&mut self, dt: f32, phrase_flipped: bool) {
        if !self.performance_enabled() || self.performance_runtime.rows.is_empty() {
            return;
        }
        let dt = f64::from(dt).clamp(0.0, 0.1);
        if dt <= 0.0 {
            return;
        }
        let row_names = self.scenes.row_names();
        let disabled = self.scenes.disabled_cells();
        let order = self
            .scenes
            .performance()
            .get("order")
            .and_then(Value::as_str)
            .unwrap_or("in-order")
            .to_string();
        let mut changed = false;
        for row in 0..self.performance_runtime.rows.len() {
            let (dwell, target, scene_id) = {
                let runtime = &mut self.performance_runtime.rows[row];
                runtime.dwell += dt;
                (runtime.dwell, runtime.target, runtime.scene_id.clone())
            };
            let timing = scene_id
                .as_deref()
                .and_then(|id| self.scenes.find(id))
                .map(|scene| {
                    resolve_scene_timing_at(Some(scene), Some(self.scenes.performance()), self.bpm)
                });
            let decision = {
                let runtime = &mut self.performance_runtime;
                should_advance_row(
                    &ShouldAdvanceRow {
                        dwell,
                        min_seconds: timing.as_ref().map(|timing| timing.min_seconds),
                        max_seconds: timing.as_ref().map(|timing| timing.max_seconds),
                        target_seconds: Some(target),
                        phrase_flipped,
                    },
                    || runtime.next_random(),
                )
            };
            if !decision.advance {
                continue;
            }
            let current = self.performance_runtime.rows[row].scene_id.clone();
            let picked = {
                let args = PickNextRowScene {
                    scenes: self.scenes.scenes(),
                    row_index: row,
                    current_scene_id: current.as_deref(),
                    order: &order,
                    row_names: &row_names,
                };
                pick_next_row_scene_with_disabled(&args, &disabled, || {
                    self.performance_runtime.next_random()
                })
                .cloned()
            };
            let Some(scene) = picked else {
                continue;
            };
            let Some(scene_id) = scene.get("id").and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            let old_row = self.capture_row_group(row);
            if !self.scenes.apply_row(
                &scene_id,
                row,
                &self.registry,
                &mut self.state,
                &mut self.tweens,
                self.bpm,
            ) {
                continue;
            }
            if let Some((old_group, old_modules)) = old_row {
                self.begin_row_transition(row, old_group, old_modules);
            }
            self.reset_performance_row(row, &scene);
            self.performance_runtime
                .active
                .insert(row, scene_id.clone());
            self.scenes.set_current_scene(Some(&scene_id));
            changed = true;
        }
        if changed {
            self.dirty = true;
        }
    }

    fn reset_performance_row(&mut self, row: usize, scene: &Value) {
        while self.performance_runtime.rows.len() <= row {
            self.performance_runtime
                .rows
                .push(PerformanceRow::default());
        }
        let timing =
            resolve_scene_timing_at(Some(scene), Some(self.scenes.performance()), self.bpm);
        let roll = self.performance_runtime.next_random();
        let target = pick_cell_dwell_numeric(
            Some(timing.min_seconds),
            Some(timing.max_seconds),
            Some(roll),
        );
        let runtime = &mut self.performance_runtime.rows[row];
        runtime.scene_id = scene.get("id").and_then(Value::as_str).map(str::to_string);
        runtime.dwell = 0.0;
        runtime.target = target;
    }

    /// A whole-Scene apply marks every row that Scene covers as live.
    fn sync_performance_scene(&mut self, scene_id: &str) {
        self.scenes.set_current_scene(Some(scene_id));
        if !self.performance_enabled() {
            return;
        }
        let row_count = self.scenes.row_count();
        let scene = self.scenes.find(scene_id).cloned();
        self.performance_runtime.active.clear();
        let Some(scene) = scene else {
            return;
        };
        for row in 0..row_count {
            let has_group = scene
                .get("groups")
                .and_then(Value::as_array)
                .and_then(|groups| groups.get(row))
                .is_some_and(|group| !group.is_null());
            if has_group {
                self.performance_runtime
                    .active
                    .insert(row, scene_id.to_string());
                self.reset_performance_row(row, &scene);
            }
        }
    }

    /// One row applied manually becomes that row's live Scene.
    fn sync_performance_row(&mut self, scene_id: &str, row: usize) {
        self.scenes.set_current_scene(Some(scene_id));
        if !self.performance_enabled() {
            return;
        }
        if let Some(scene) = self.scenes.find(scene_id).cloned() {
            self.reset_performance_row(row, &scene);
            self.performance_runtime
                .active
                .insert(row, scene_id.to_string());
        }
    }

    /// Pick the transition name the native shell would use for this change.
    fn available_transition_names(&self) -> Vec<String> {
        let mut names = vec![FADE_TRANSITION.to_string(), NONE_TRANSITION.to_string()];
        names.extend(NATIVE_TRANSITION_NAMES.iter().map(|name| name.to_string()));
        names.extend(
            self.registry
                .isf_definitions()
                .iter()
                .filter(|definition| definition.document.filter_type() == "transition")
                .map(|definition| definition.name.clone()),
        );
        names
    }

    fn pick_scene_transition_name(&mut self) -> String {
        let enabled = self.performance_enabled();
        let allowed =
            normalize_transition_list(self.scenes.performance().get("allowedTransitions"));
        let available = self.available_transition_names();
        let roll = self.performance_runtime.next_random();
        pick_transition_name(enabled, &allowed, &available, || roll)
    }

    /// Keep the outgoing Groups alive for a whole-Scene Fade or shader
    /// transition. Called after the incoming Scene has been applied.
    fn begin_whole_scene_transition(
        &mut self,
        old_groups: Vec<Group>,
        old_modules: HashMap<String, ModuleInstance>,
    ) {
        self.finish_whole_transition(true);
        self.finish_all_row_transitions();
        if old_groups.is_empty() {
            return;
        }
        let mut name = self.pick_scene_transition_name();
        let effect = TransitionEffect::from_name(&name, &self.registry);
        if effect.is_none() && name != FADE_TRANSITION && name != NONE_TRANSITION {
            name = FADE_TRANSITION.to_string();
        }
        if name == NONE_TRANSITION {
            return;
        }
        let incoming: HashSet<String> = self.state.groups.iter().map(|g| g.id.clone()).collect();
        if old_groups.iter().any(|group| incoming.contains(&group.id)) {
            // Re-applying the same Scene reuses document ids; a ghost would
            // collide with its replacement, so switch instantly.
            return;
        }
        let from: Vec<(String, f64)> = old_groups
            .iter()
            .map(|group| (group.id.clone(), group.alpha))
            .collect();
        let to: Vec<(String, f64)> = self
            .state
            .groups
            .iter()
            .map(|group| (group.id.clone(), group.alpha))
            .collect();
        for (id, module) in old_modules {
            self.state.modules.entry(id).or_insert(module);
        }
        let mut ghosts = old_groups;
        if effect.is_some() {
            for group in &mut ghosts {
                group.hidden = true;
                group.transition_ghost = None;
            }
        }
        self.state.groups.splice(0..0, ghosts);
        if effect.is_none() {
            for (id, _) in &to {
                if let Some(group) = self.state.find_group_mut(id) {
                    group.alpha = 0.0;
                }
            }
        } else {
            self.pending_transition_capture = effect.clone();
        }
        let duration =
            normalize_transition_duration(self.scenes.performance().get("transitionSeconds"));
        self.transition = Some(WholeTransition {
            from,
            to,
            effect,
            elapsed: 0.0,
            duration,
        });
    }

    /// Keep one outgoing row Group as a `transitionGhost` for a crossfade.
    fn begin_row_transition(
        &mut self,
        row: usize,
        old_group: Group,
        old_modules: HashMap<String, ModuleInstance>,
    ) {
        self.finish_row_transition(row, true);
        let Some(new_group_id) = self
            .state
            .groups
            .iter()
            .find(|group| group.scene_row == Some(row) && group.transition_ghost != Some(true))
            .map(|group| group.id.clone())
        else {
            return;
        };
        if old_group.id == new_group_id {
            return;
        }
        let target_alpha = self
            .state
            .find_group(&new_group_id)
            .map(|group| group.alpha)
            .unwrap_or(1.0);
        let from_alpha = old_group.alpha;
        for (id, module) in old_modules {
            self.state.modules.entry(id).or_insert(module);
        }
        let mut ghost = old_group;
        ghost.transition_ghost = Some(true);
        ghost.hidden = false;
        let insert_at = self
            .state
            .groups
            .iter()
            .position(|group| group.id == new_group_id)
            .unwrap_or(self.state.groups.len());
        let ghost_id = ghost.id.clone();
        self.state.groups.insert(insert_at, ghost);
        if let Some(group) = self.state.find_group_mut(&new_group_id) {
            group.alpha = 0.0;
        }
        let duration =
            normalize_transition_duration(self.scenes.performance().get("transitionSeconds"));
        self.row_transitions.insert(
            row,
            RowTransition {
                row,
                group_id: new_group_id,
                ghost_group_id: ghost_id,
                from_alpha,
                target_alpha,
                elapsed: 0.0,
                duration,
            },
        );
    }

    /// Advance every in-flight transition. Called once per rendered frame.
    pub fn tick_transitions(&mut self, dt: f32) {
        let dt = f64::from(dt).clamp(0.0, 0.1);
        if dt <= 0.0 {
            return;
        }
        if let Some(transition) = self.transition.as_mut() {
            transition.elapsed += dt;
            if transition.effect.is_none() {
                let eased = smoothstep(transition.progress());
                for (id, from) in &transition.from {
                    if let Some(group) = self.state.find_group_mut(id) {
                        group.alpha = from * (1.0 - eased);
                    }
                }
                for (id, target) in &transition.to {
                    if let Some(group) = self.state.find_group_mut(id) {
                        group.alpha = target * eased;
                    }
                }
            }
            if transition.progress() >= 1.0 {
                self.finish_whole_transition(true);
            }
        }
        let rows: Vec<usize> = self.row_transitions.keys().copied().collect();
        for row in rows {
            let finished = if let Some(transition) = self.row_transitions.get_mut(&row) {
                transition.elapsed += dt;
                let eased = smoothstep(transition.progress());
                if let Some(group) = self.state.find_group_mut(&transition.ghost_group_id) {
                    group.alpha = transition.from_alpha * (1.0 - eased);
                }
                if let Some(group) = self.state.find_group_mut(&transition.group_id) {
                    group.alpha = transition.target_alpha * eased;
                }
                transition.progress() >= 1.0
            } else {
                false
            };
            if finished {
                self.finish_row_transition(row, true);
            }
        }
    }

    fn finish_whole_transition(&mut self, remove_outgoing: bool) {
        let Some(transition) = self.transition.take() else {
            return;
        };
        if remove_outgoing {
            for (id, _) in &transition.from {
                self.remove_group_and_modules(id);
            }
        }
        for (id, target) in &transition.to {
            if let Some(group) = self.state.find_group_mut(id) {
                group.alpha = *target;
            }
        }
        self.pending_transition_capture = None;
    }

    fn finish_row_transition(&mut self, row: usize, remove_ghost: bool) {
        let Some(transition) = self.row_transitions.remove(&row) else {
            return;
        };
        if remove_ghost {
            self.remove_group_and_modules(&transition.ghost_group_id);
        }
        if let Some(group) = self.state.find_group_mut(&transition.group_id) {
            group.alpha = transition.target_alpha;
        }
    }

    fn finish_all_row_transitions(&mut self) {
        let rows: Vec<usize> = self.row_transitions.keys().copied().collect();
        for row in rows {
            self.finish_row_transition(row, true);
        }
    }

    fn remove_group_and_modules(&mut self, group_id: &str) {
        let Some(position) = self
            .state
            .groups
            .iter()
            .position(|group| group.id == group_id)
        else {
            return;
        };
        let group = self.state.groups.remove(position);
        for module_id in group.modules {
            let still_used = self
                .state
                .groups
                .iter()
                .any(|group| group.modules.iter().any(|id| id == &module_id));
            if !still_used {
                self.state.modules.shift_remove(&module_id);
            }
        }
    }

    /// The shader transition to hand to the compositor this frame.
    pub fn scene_transition_info(&self) -> Option<SceneTransitionInfo> {
        let transition = self.transition.as_ref()?;
        let effect = transition.effect.clone()?;
        Some(SceneTransitionInfo {
            effect,
            progress: transition.progress(),
            from_groups: transition
                .from
                .iter()
                .map(|(group_id, _)| group_id.clone())
                .collect(),
        })
    }

    /// Per-row crossfade descriptions for `FrameInfo::row_fades`.
    pub fn row_fade_infos(&self) -> Vec<RowFadeInfo> {
        self.row_transitions
            .values()
            .map(|transition| RowFadeInfo {
                row: transition.row,
                group_id: transition.group_id.clone(),
                ghost_group_id: transition.ghost_group_id.clone(),
                from_alpha: transition.from_alpha,
                target_alpha: transition.target_alpha,
                progress: transition.progress(),
            })
            .collect()
    }

    /// Take the capture request a shader transition left for the app.
    pub fn take_pending_transition_capture(&mut self) -> Option<TransitionEffect> {
        self.pending_transition_capture.take()
    }

    /// Downgrade a shader transition to the Fade alpha path when the outgoing
    /// frame or its pipeline is unavailable.
    pub fn fall_back_transition_to_fade(&mut self) -> bool {
        let Some(transition) = self.transition.as_mut() else {
            return false;
        };
        if transition.effect.is_none() {
            return false;
        }
        transition.effect = None;
        self.pending_transition_capture = None;
        for (id, from) in &transition.from {
            if let Some(group) = self.state.find_group_mut(id) {
                group.hidden = false;
                group.alpha = *from;
            }
        }
        for (id, _) in &transition.to {
            if let Some(group) = self.state.find_group_mut(id) {
                group.alpha = 0.0;
            }
        }
        true
    }

    fn capture_live_groups(&self) -> (Vec<Group>, HashMap<String, ModuleInstance>) {
        let groups: Vec<Group> = self
            .state
            .groups
            .iter()
            .filter(|group| group.scene_id.is_some() && group.transition_ghost != Some(true))
            .cloned()
            .collect();
        let mut modules = HashMap::new();
        for group in &groups {
            for module_id in &group.modules {
                if let Some(module) = self.state.modules.get(module_id) {
                    modules.insert(module_id.clone(), module.clone());
                }
            }
        }
        (groups, modules)
    }

    fn capture_row_group(&self, row: usize) -> Option<(Group, HashMap<String, ModuleInstance>)> {
        let group = self
            .state
            .groups
            .iter()
            .filter(|group| group.transition_ghost != Some(true))
            .find(|group| group.scene_row == Some(row))
            .or_else(|| {
                self.state
                    .groups
                    .iter()
                    .filter(|group| group.transition_ghost != Some(true))
                    .nth(row)
            })?
            .clone();
        let modules = group
            .modules
            .iter()
            .filter_map(|module_id| {
                self.state
                    .modules
                    .get(module_id)
                    .map(|module| (module_id.clone(), module.clone()))
            })
            .collect();
        Some((group, modules))
    }

    pub fn apply_action(&mut self, action: &UiAction) -> bool {
        let changed = self.apply_action_inner(action);
        if changed {
            self.dirty = true;
        }
        changed
    }

    fn apply_action_inner(&mut self, action: &UiAction) -> bool {
        match action.action.as_str() {
            "ui/addModuleToGroup" => {
                let module = action.payload.get("moduleName").and_then(Value::as_str);
                let group = action.payload.get("groupId").and_then(Value::as_str);
                match (module, group) {
                    (Some(module), Some(group)) => {
                        self.add_module_to_group(module, group).is_some()
                    }
                    (Some(module), None) => self.add_module(module).is_some(),
                    _ => false,
                }
            }
            "modules/updateProp" => {
                let Some(module_id) = action.payload.get("moduleId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(prop) = action.payload.get("prop").and_then(Value::as_str) else {
                    return false;
                };
                let Some(value) = action
                    .payload
                    .get("data")
                    .and_then(|data| data.get("value"))
                else {
                    return false;
                };
                self.update_prop(module_id, prop, value.clone())
            }
            "modules/updateMeta" => {
                let Some(module_id) = action.payload.get("moduleId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(key) = action.payload.get("metaKey").and_then(Value::as_str) else {
                    return false;
                };
                let Some(value) = action.payload.get("value") else {
                    return false;
                };
                self.update_meta(module_id, key, value.clone())
            }
            "modules/removeActiveModule" => action
                .payload
                .get("moduleId")
                .and_then(Value::as_str)
                .is_some_and(|id| self.remove_module(id)),
            "groups/updateGroupName" => {
                let Some(group_id) = action.payload.get("groupId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(name) = action.payload.get("name").and_then(Value::as_str) else {
                    return false;
                };
                let Some(group) = self.state.find_group_mut(group_id) else {
                    return false;
                };
                group.name = name.to_string();
                true
            }
            "groups/UPDATE_GROUP_BY_KEY" | "groups/updateGroupByKey" => {
                self.update_group_by_key(&action.payload)
            }
            "groups/createGroup" => {
                self.next_id = self.next_id.saturating_add(1);
                let id = format!("web-group-{}", self.next_id);
                let name = action
                    .payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("New Group");
                let enabled = action
                    .payload
                    .get("enabled")
                    .and_then(Value::as_i64)
                    .unwrap_or(GROUP_DISABLED);
                self.state
                    .groups
                    .push(new_group(&id, name, enabled.clamp(0, 2)));
                self.install_group_inputs(&id);
                true
            }
            "groups/removeGroup" => {
                let Some(group_id) = action.payload.get("groupId").and_then(Value::as_str) else {
                    return false;
                };
                if group_id == self.group_id {
                    return false;
                }
                let Some(group) = self
                    .state
                    .groups
                    .iter()
                    .find(|group| group.id == group_id)
                    .cloned()
                else {
                    return false;
                };
                for module_id in &group.modules {
                    self.state.modules.shift_remove(module_id);
                }
                self.state.groups.retain(|group| group.id != group_id);
                true
            }
            "groups/REMOVE_MODULE_FROM_GROUP" => {
                let Some(group_id) = action.payload.get("groupId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(module_id) = action.payload.get("moduleId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(group) = self.state.find_group_mut(group_id) else {
                    return false;
                };
                group.modules.retain(|id| id != module_id);
                self.state.modules.shift_remove(module_id);
                true
            }
            "groups/MOVE_MODULE" => {
                let Some(module_id) = action.payload.get("moduleId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(to_group) = action.payload.get("toGroupId").and_then(Value::as_str) else {
                    return false;
                };
                let to_index = action
                    .payload
                    .get("toIndex")
                    .and_then(Value::as_u64)
                    .unwrap_or(u64::MAX) as usize;
                for group in &mut self.state.groups {
                    group.modules.retain(|id| id != module_id);
                }
                let Some(group) = self.state.find_group_mut(to_group) else {
                    return false;
                };
                let index = to_index.min(group.modules.len());
                group.modules.insert(index, module_id.to_string());
                true
            }
            "groups/duplicateModule" | "modules/duplicateModule" => {
                self.duplicate_module(&action.payload)
            }
            "inputs/createInputLink" => self.upsert_input_link(&action.payload),
            "inputs/updateInputLink" => self.update_input_link(&action.payload),
            "inputs/removeInputLink" => action
                .payload
                .get("inputId")
                .and_then(Value::as_str)
                .is_some_and(|id| self.state.input_links.shift_remove(id).is_some()),
            "meyda/removeSmoothingId" => action
                .payload
                .as_str()
                .is_some_and(|id| self.smoothing.remove(id).is_some()),
            "tweens/createTween" | "tweens/ADD_TWEEN" => self.create_tween(&action.payload),
            "tweens/REMOVE_TWEEN" => action
                .payload
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| self.remove_tween(id)),
            "groups/orderByIds" => {
                let Some(ids) = action.payload.get("ids").and_then(Value::as_array) else {
                    return false;
                };
                let order: Vec<&str> = ids.iter().filter_map(Value::as_str).collect();
                self.state.groups.sort_by_key(|group| {
                    order
                        .iter()
                        .position(|id| *id == group.id)
                        .unwrap_or(usize::MAX)
                });
                true
            }
            "scenes/applyScene" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let (old_groups, old_modules) = self.capture_live_groups();
                let Some(group_id) = self.scenes.apply(
                    scene_id,
                    &self.registry,
                    &mut self.state,
                    &mut self.tweens,
                    self.bpm,
                ) else {
                    return false;
                };
                self.group_id = group_id;
                self.begin_whole_scene_transition(old_groups, old_modules);
                self.sync_performance_scene(scene_id);
                true
            }
            "scenes/applySceneRow" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(row) = action.payload.get("rowIndex").and_then(Value::as_u64) else {
                    return false;
                };
                let old_row = self.capture_row_group(row as usize);
                let changed = self.scenes.apply_row(
                    scene_id,
                    row as usize,
                    &self.registry,
                    &mut self.state,
                    &mut self.tweens,
                    self.bpm,
                );
                if changed {
                    if let Some((old_group, old_modules)) = old_row {
                        self.begin_row_transition(row as usize, old_group, old_modules);
                    }
                    self.sync_performance_row(scene_id, row as usize);
                }
                changed
            }
            "scenes/addEmptyScene" => {
                let name = action.payload.get("name").and_then(Value::as_str);
                self.scenes.add_empty(name);
                true
            }
            "scenes/updateSceneName" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(name) = action.payload.get("name").and_then(Value::as_str) else {
                    return false;
                };
                self.scenes.rename(scene_id, name)
            }
            "scenes/removeScene" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let removed = self.scenes.remove(scene_id);
                if removed {
                    self.drop_scene_groups(scene_id);
                    self.performance_runtime
                        .active
                        .retain(|_, id| id != scene_id);
                }
                removed
            }
            "scenes/moveScene" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(to_index) = action.payload.get("toIndex").and_then(Value::as_i64) else {
                    return false;
                };
                self.scenes.move_to(scene_id, to_index)
            }
            "scenes/setSceneEnabled" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(enabled) = action.payload.get("enabled").and_then(Value::as_bool) else {
                    return false;
                };
                self.scenes.set_enabled(scene_id, enabled)
            }
            "scenes/updateSceneTiming" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(min) = action.payload.get("minSeconds").and_then(Value::as_f64) else {
                    return false;
                };
                let Some(max) = action.payload.get("maxSeconds").and_then(Value::as_f64) else {
                    return false;
                };
                self.scenes.set_timing(scene_id, min, max)
            }
            "scenes/resetSceneTiming" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                self.scenes.reset_timing(scene_id)
            }
            "scenes/setCellEnabled" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(row) = action.payload.get("rowIndex").and_then(Value::as_u64) else {
                    return false;
                };
                let enabled = action
                    .payload
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.scenes
                    .set_cell_enabled(scene_id, row as usize, enabled)
            }
            "scenes/clearSceneCell" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(row) = action.payload.get("rowIndex").and_then(Value::as_u64) else {
                    return false;
                };
                self.scenes.clear_cell(scene_id, row as usize)
            }
            "scenes/copySceneCellToScene" => {
                let Some(from_scene) = action.payload.get("fromSceneId").and_then(Value::as_str)
                else {
                    return false;
                };
                let Some(from_row) = action.payload.get("fromRowIndex").and_then(Value::as_u64)
                else {
                    return false;
                };
                let Some(to_scene) = action.payload.get("toSceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(to_row) = action.payload.get("toRowIndex").and_then(Value::as_u64) else {
                    return false;
                };
                self.scenes
                    .copy_cell(from_scene, from_row as usize, to_scene, to_row as usize)
            }
            "scenes/createSceneCellGroup" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(row) = action.payload.get("rowIndex").and_then(Value::as_u64) else {
                    return false;
                };
                self.scenes.create_cell_group(scene_id, row as usize)
            }
            "scenes/addModuleToCell" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(row) = action.payload.get("rowIndex").and_then(Value::as_u64) else {
                    return false;
                };
                let Some(module_name) = action.payload.get("moduleName").and_then(Value::as_str)
                else {
                    return false;
                };
                self.scenes
                    .add_module_to_cell(&self.registry, scene_id, row as usize, module_name)
            }
            "scenes/setSceneCellGroup" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(row) = action.payload.get("rowIndex").and_then(Value::as_u64) else {
                    return false;
                };
                let Some(group_id) = action.payload.get("groupId").and_then(Value::as_str) else {
                    return false;
                };
                self.scenes.set_cell_group(
                    scene_id,
                    row as usize,
                    &self.state,
                    group_id,
                    &self.tweens,
                )
            }
            "scenes/captureLiveStateToScene" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                self.scenes
                    .capture_live_state(scene_id, &self.state, &self.tweens)
            }
            "scenes/captureSceneRowToScene" => {
                let Some(scene_id) = action.payload.get("sceneId").and_then(Value::as_str) else {
                    return false;
                };
                let Some(row) = action.payload.get("rowIndex").and_then(Value::as_u64) else {
                    return false;
                };
                self.scenes
                    .capture_live_row(scene_id, row as usize, &self.state, &self.tweens)
            }
            "scenes/updatePerformance" => {
                let config = action
                    .payload
                    .get("config")
                    .and_then(Value::as_object)
                    .or_else(|| action.payload.as_object());
                let Some(config) = config else {
                    return false;
                };
                self.scenes.update_performance(config)
            }
            "scenes/setRowNames" => {
                let Some(names) = action.payload.get("rowNames") else {
                    return false;
                };
                self.scenes.set_row_names(names)
            }
            "scenes/setPerformanceEnabled" => {
                let Some(enabled) = action.payload.get("enabled").and_then(Value::as_bool) else {
                    return false;
                };
                let changed = self.scenes.set_performance_enabled(enabled);
                if enabled {
                    self.start_performance();
                } else {
                    self.stop_performance();
                }
                changed
            }
            "scenes/setTimingUnit" => {
                let unit = action
                    .payload
                    .get("unit")
                    .or_else(|| action.payload.get("timingUnit"))
                    .and_then(Value::as_str)
                    .unwrap_or("seconds");
                self.scenes.set_timing_unit(unit, self.bpm)
            }
            // Selection-only, media, audio-device, NDI/OSC/MIDI and plugin
            // actions are outside the web shell's cut line.
            _ => false,
        }
    }

    /// Remove the live Groups a deleted Scene owned.
    fn drop_scene_groups(&mut self, scene_id: &str) {
        let removed: Vec<String> = self
            .state
            .groups
            .iter()
            .filter(|group| group.scene_id.as_deref() == Some(scene_id))
            .flat_map(|group| group.modules.clone())
            .collect();
        self.state
            .groups
            .retain(|group| group.scene_id.as_deref() != Some(scene_id));
        for module_id in removed {
            self.state.modules.shift_remove(&module_id);
            self.smoothing.remove(&module_id);
        }
        if self.state.find_group(&self.group_id).is_none() {
            self.group_id = self
                .state
                .groups
                .first()
                .map(|group| group.id.clone())
                .unwrap_or_default();
        }
    }

    fn update_group_by_key(&mut self, payload: &Value) -> bool {
        let Some(group_id) = payload.get("groupId").and_then(Value::as_str) else {
            return false;
        };
        let Some(key) = payload.get("key").and_then(Value::as_str) else {
            return false;
        };
        let Some(value) = payload.get("value") else {
            return false;
        };
        let Some(group) = self.state.find_group_mut(group_id) else {
            return false;
        };
        match key {
            "name" => {
                let Some(name) = value.as_str() else {
                    return false;
                };
                group.name = name.to_string();
            }
            "alpha" => {
                let Some(alpha) = value.as_f64() else {
                    return false;
                };
                group.alpha = alpha.clamp(0.0, 1.0);
            }
            "enabled" => {
                let Some(enabled) = value.as_i64() else {
                    return false;
                };
                group.enabled = enabled.clamp(0, 2);
            }
            "clearing" => {
                let Some(clearing) = value.as_bool() else {
                    return false;
                };
                group.clearing = clearing;
            }
            "inherit" => {
                let Some(inherit) = value.as_bool() else {
                    return false;
                };
                group.inherit = inherit;
            }
            "inheritFrom" => {
                group.inherit_from = match value {
                    Value::String(_) | Value::Number(_) => InheritFrom::from_wire(Some(value)),
                    _ => return false,
                };
            }
            "pipeline" => {
                let Some(pipeline) = value.as_bool() else {
                    return false;
                };
                group.pipeline = pipeline;
            }
            "compositeOperation" => {
                let Some(operation) = value.as_str() else {
                    return false;
                };
                group.composite_operation = operation.to_string();
            }
            "hidden" => {
                let Some(hidden) = value.as_bool() else {
                    return false;
                };
                group.hidden = hidden;
            }
            _ => return false,
        }
        true
    }

    fn duplicate_module(&mut self, payload: &Value) -> bool {
        let Some(module_id) = payload.get("moduleId").and_then(Value::as_str) else {
            return false;
        };
        let Some(source) = self.state.modules.get(module_id).cloned() else {
            return false;
        };
        let Some(group_id) = self
            .state
            .groups
            .iter()
            .find(|group| group.modules.iter().any(|id| id == module_id))
            .map(|group| group.id.clone())
        else {
            return false;
        };
        self.next_id = self.next_id.saturating_add(1);
        let id = format!("web-module-{}", self.next_id);
        let mut copy = source;
        copy.id = id.clone();
        let name = copy
            .meta
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&copy.module_name)
            .to_string();
        if let Some(meta) = copy.meta.as_object_mut() {
            meta.insert("name".to_string(), json!(format!("{name} (copy)")));
        }
        copy.dollar_props = json!({});
        if let Some(meta) = copy.meta.as_object_mut() {
            meta.remove("alphaInputId");
            meta.remove("enabledInputId");
            meta.remove("compositeOperationInputId");
        }
        let module_name = copy.module_name.clone();
        self.state.modules.insert(id.clone(), copy);
        self.install_module_inputs(&id, &module_name);
        if let Some(group) = self.state.find_group_mut(&group_id) {
            let index = group
                .modules
                .iter()
                .position(|existing| existing == module_id)
                .map(|index| index + 1)
                .unwrap_or(group.modules.len());
            group.modules.insert(index, id);
        }
        true
    }

    /// Build the read model the existing `modv-ui` shell draws.
    pub fn snapshot(&self, resolution: [u32; 2], fps: f32, frame_ms: f32) -> EngineSnapshot {
        let groups = self
            .state
            .groups
            .iter()
            .filter(|group| group.transition_ghost != Some(true))
            .map(group_view)
            .collect();
        let tween_by_id: HashMap<String, TweenView> = self
            .tweens
            .iter()
            .map(|(id, runtime)| {
                let descriptor = runtime.descriptor();
                (
                    id.to_string(),
                    TweenView {
                        id: id.to_string(),
                        data: descriptor.data.clone(),
                        keyframe_times: descriptor.effective_keyframe_times().map(<[f64]>::to_vec),
                        duration: descriptor.duration,
                        direction: descriptor.direction.clone(),
                        easing: descriptor.easing.clone(),
                        looping: descriptor.looping,
                        use_bpm: descriptor.use_bpm,
                        bpm_division: descriptor.bpm_division,
                        duration_as_total_time: descriptor.duration_as_total_time,
                        steps: descriptor.steps,
                        frames: runtime.frames().len() as u64,
                        cycle_seconds: descriptor.effective_duration(self.bpm) / 1000.0,
                    },
                )
            })
            .collect();
        let visible_modules: HashSet<&str> = self
            .state
            .groups
            .iter()
            .filter(|group| group.transition_ghost != Some(true))
            .flat_map(|group| group.modules.iter().map(String::as_str))
            .collect();
        let modules = self
            .state
            .modules
            .values()
            .filter(|module| visible_modules.contains(module.id.as_str()))
            .map(|module| {
                let group_id = self
                    .state
                    .groups
                    .iter()
                    .find(|group| group.modules.iter().any(|id| id == &module.id))
                    .map(|group| group.id.clone())
                    .unwrap_or_default();
                module_view(module, group_id, &self.registry, &tween_by_id)
            })
            .collect();
        let input_links = self
            .state
            .input_links
            .iter()
            .map(|(input_id, link)| {
                (
                    input_id.clone(),
                    InputLinkView {
                        input_id: input_id.clone(),
                        kind: format!("{:?}", link.kind).to_ascii_lowercase(),
                        location: link.location.clone(),
                        args: link
                            .args
                            .as_ref()
                            .map(|args| serde_json::to_value(args).unwrap_or(Value::Null)),
                        source: link.source.clone(),
                        min: link.min,
                        max: link.max,
                        matcher: link.matcher.clone(),
                    },
                )
            })
            .collect();
        let registered = registered_modules(&self.registry);
        let mut performance = self.scenes.performance_view();
        performance.available_transitions = self.available_transition_names();
        let scenes = self.scenes.views();
        let scene_rows = self.scenes.row_names();
        let applied_scene_id = self.scenes.current_scene_id();
        let active_scene_rows = self.performance_runtime.active.clone();
        let audio = AudioView {
            bpm: self.bpm,
            kick: self.audio.beat >= 0.5,
            bass: self.audio.bass,
            mid: self.audio.mid,
            treble: self.audio.treble,
            sample_rate: Some(self.audio.sample_rate.round() as u32),
        };
        let health = EngineHealth {
            adapter: self.adapter.clone(),
            backend: self.backend.clone(),
            last_error: self.error.clone(),
            frames_presented: self.frames,
            output_open: true,
            output_fullscreen: false,
            audio_active: audio.bass + audio.mid + audio.treble > 0.0,
        };
        EngineSnapshot {
            groups,
            modules,
            registered,
            performance,
            scenes,
            scene_rows,
            applied_scene_id,
            active_scene_rows,
            audio,
            resolution,
            fps,
            frame_ms,
            health,
            input_links,
            tweens: tween_by_id,
            ..EngineSnapshot::default()
        }
    }
}

/// The checked-in ISF corpus, compiled into the wasm binary.
///
/// The browser has no filesystem scanner, so the same `resources/isf` tree
/// the desktop app ships is embedded here. Parsing is header-only.
static ISF_CORPUS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../resources/isf");
static BUNDLED_REGISTRY: OnceLock<Arc<ModuleRegistry>> = OnceLock::new();

fn bundled_registry() -> Arc<ModuleRegistry> {
    BUNDLED_REGISTRY
        .get_or_init(|| {
            let mut files: Vec<_> = ISF_CORPUS.files().collect();
            files.sort_by(|left, right| left.path().cmp(right.path()));
            let mut sources = Vec::with_capacity(files.len());
            for file in files {
                let path = file.path();
                if path.extension() != Some(OsStr::new("fs")) {
                    continue;
                }
                let Some(name) = path.file_stem().and_then(OsStr::to_str) else {
                    continue;
                };
                let Some(fragment) = file.contents_utf8() else {
                    continue;
                };
                let vertex = ISF_CORPUS
                    .get_file(path.with_extension("vs"))
                    .and_then(|file| file.contents_utf8())
                    .map(str::to_string);
                sources.push((name.to_string(), fragment.to_string(), vertex));
            }
            Arc::new(ModuleRegistry::builtin().with_isf_sources(&sources))
        })
        .clone()
}

fn new_group(id: &str, name: &str, enabled: i64) -> Group {
    Group {
        id: id.to_string(),
        name: name.to_string(),
        hidden: false,
        modules: Vec::new(),
        enabled,
        alpha: 1.0,
        inherit: true,
        inherit_from: InheritFrom::Previous(-1),
        pipeline: false,
        clearing: true,
        composite_operation: "normal".to_string(),
        scene_id: None,
        scene_row: None,
        transition_ghost: None,
        draw_to_canvas_id: None,
        alpha_input_id: None,
        enabled_input_id: None,
        clearing_input_id: None,
        inherit_input_id: None,
        composite_operation_input_id: None,
        pipeline_input_id: None,
        extra: Map::new(),
    }
}

fn group_view(group: &Group) -> GroupView {
    GroupView {
        id: group.id.clone(),
        name: group.name.clone(),
        enabled: group.enabled,
        hidden: group.hidden,
        alpha: group.alpha as f32,
        alpha_input_id: group.alpha_input_id.clone(),
        composite_operation: group.composite_operation.clone(),
        composite_operation_input_id: group.composite_operation_input_id.clone(),
        inherit: group.inherit,
        inherit_from: match &group.inherit_from {
            InheritFrom::Previous(_) => InheritTarget::Previous,
            InheritFrom::Group(id) => InheritTarget::Group(id.clone()),
            InheritFrom::Composition => InheritTarget::Composition,
        },
        clearing: group.clearing,
        pipeline: group.pipeline,
        module_ids: group.modules.clone(),
    }
}

fn module_view(
    module: &ModuleInstance,
    group_id: String,
    registry: &ModuleRegistry,
    tween_by_id: &HashMap<String, TweenView>,
) -> ModuleView {
    let meta = module.meta.as_object();
    let module_type = registry
        .meta_fields(&module.module_name)
        .and_then(|meta| meta.get("type").and_then(Value::as_str).map(str::to_string))
        .or_else(|| {
            module
                .meta
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let meta_string = |key: &str| {
        meta.and_then(|meta| meta.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let props = module
        .props
        .as_object()
        .map(|props| {
            props
                .iter()
                .map(|(name, value)| {
                    let schema = registry.prop_schema(&module.module_name, name);
                    let prop_type = schema
                        .as_ref()
                        .map(|schema| schema.type_.clone())
                        .unwrap_or_else(|| "text".to_string());
                    let input_id = module
                        .dollar_props
                        .as_object()
                        .and_then(|props| props.get(name))
                        .and_then(|entry| entry.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    PropView {
                        name: name.clone(),
                        step: Some(if prop_type == "int" { 1.0 } else { 0.001 }),
                        prop_type,
                        value: value.clone(),
                        min: schema.as_ref().and_then(|schema| schema.min),
                        max: schema.as_ref().and_then(|schema| schema.max),
                        default: schema.as_ref().and_then(|schema| schema.default.clone()),
                        input_id: input_id.clone(),
                        components: Vec::new(),
                        tween: input_id
                            .as_ref()
                            .and_then(|id| tween_by_id.get(id))
                            .cloned(),
                        enum_members: schema
                            .as_ref()
                            .and_then(|schema| schema.enum_members.as_ref())
                            .map(|members| {
                                members
                                    .iter()
                                    .map(|member| PropEnumMember {
                                        label: member.label.clone(),
                                        value: member.value.clone(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    ModuleView {
        id: module.id.clone(),
        module_name: module.module_name.clone(),
        group_id,
        enabled: meta
            .and_then(|meta| meta.get("enabled"))
            .map(js_truthy)
            .unwrap_or(true),
        alpha: meta
            .and_then(|meta| meta.get("alpha"))
            .and_then(Value::as_f64)
            .unwrap_or(1.0) as f32,
        alpha_input_id: meta_string("alphaInputId"),
        composite_operation: meta_string("compositeOperation")
            .unwrap_or_else(|| "normal".to_string()),
        composite_operation_input_id: meta_string("compositeOperationInputId"),
        module_type,
        props,
    }
}

fn registered_modules(registry: &ModuleRegistry) -> Vec<ModuleMetaView> {
    let Value::Object(map) = registry.registered_modules_ref() else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, entry)| {
            let meta = entry.get("meta").and_then(Value::as_object)?;
            let get = |key: &str| {
                meta.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let kind = registry
                .kind(name)
                .map(|kind| kind.as_str().to_string())
                .unwrap_or_default();
            Some(ModuleMetaView {
                name: name.clone(),
                module_type: get("type"),
                kind,
                category: get("category"),
                author: get("author"),
                version: get("version"),
                description: get("description"),
            })
        })
        .collect()
}

fn value_to_rows(data: &[Value]) -> Vec<Vec<f64>> {
    data.iter()
        .map(|row| {
            row.as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_f64)
                        .collect::<Vec<f64>>()
                })
                .unwrap_or_default()
        })
        .collect()
}

fn push_tween_json(tweens: &mut Value, tween: Value) {
    let id = tween.get("id").and_then(Value::as_str).map(str::to_string);
    match tweens {
        Value::Array(entries) => {
            if let Some(id) = &id
                && let Some(existing) = entries
                    .iter_mut()
                    .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id.as_str()))
            {
                *existing = tween;
                return;
            }
            entries.push(tween);
        }
        Value::Object(map) => {
            if let Some(entries) = map.get_mut("tweens") {
                push_tween_json(entries, tween);
            } else if let Some(id) = id {
                map.insert(id, tween);
            }
        }
        _ => *tweens = Value::Array(vec![tween]),
    }
}

fn remove_tween_json(tweens: &mut Value, id: &str) -> bool {
    match tweens {
        Value::Array(entries) => {
            let before = entries.len();
            entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(id));
            entries.len() != before
        }
        Value::Object(map) => {
            if let Some(entries) = map.get_mut("tweens") {
                remove_tween_json(entries, id)
            } else {
                map.shift_remove(id).is_some()
            }
        }
        _ => false,
    }
}

fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_bands(state: &mut WebState) -> String {
        state
            .add_module("Bands")
            .expect("the built-in registry knows Bands")
    }

    #[test]
    fn adding_a_module_joins_the_default_group() {
        let mut state = WebState::new();
        let id = add_bands(&mut state);

        assert_eq!(state.state().modules.len(), 1);
        assert_eq!(state.state().groups.len(), 1);
        assert_eq!(state.state().groups[0].modules, vec![id.clone()]);
        let module = &state.state().modules[&id];
        assert_eq!(module.module_name, "Bands");
        assert_eq!(module.meta["enabled"], json!(true));
        assert_eq!(module.props["u_ringWidth"], json!(0.05));
    }

    #[test]
    fn updating_a_prop_lands_on_the_module() {
        let mut state = WebState::new();
        let id = add_bands(&mut state);

        assert!(state.update_prop(&id, "u_ringWidth", json!(0.25)));

        assert_eq!(state.state().modules[&id].props["u_ringWidth"], json!(0.25));
    }

    #[test]
    fn removing_a_module_clears_group_membership() {
        let mut state = WebState::new();
        let id = add_bands(&mut state);

        assert!(state.remove_module(&id));

        assert!(state.state().modules.is_empty());
        assert!(state.state().groups[0].modules.is_empty());
    }

    #[test]
    fn gallery_action_adds_a_module() {
        let mut state = WebState::new();
        let group = state.group_id().to_string();
        let action = UiAction::add_module_to_group("Bands", &group);

        assert!(state.apply_action(&action));
        assert_eq!(state.state().modules.len(), 1);
    }

    #[test]
    fn snapshot_exposes_the_live_group_and_the_registry() {
        let mut state = WebState::new();
        let id = add_bands(&mut state);

        let snapshot = state.snapshot([960, 540], 60.0, 16.0);

        assert_eq!(snapshot.resolution, [960, 540]);
        assert_eq!(snapshot.groups.len(), 1);
        assert_eq!(snapshot.groups[0].module_ids, vec![id.clone()]);
        assert_eq!(snapshot.modules.len(), 1);
        assert_eq!(snapshot.modules[0].id, id);
        assert_eq!(snapshot.modules[0].props[0].name, "u_ringWidth");
        assert!(
            snapshot
                .registered
                .iter()
                .any(|module| module.name == "Bands"),
            "the Gallery must see the built-in registry"
        );
    }

    fn prop_input_id(state: &WebState, module_id: &str, prop: &str) -> String {
        state.state().modules[module_id]
            .dollar_props
            .get(prop)
            .and_then(|descriptor| descriptor.get("id"))
            .and_then(Value::as_str)
            .expect("the module prop has an input id")
            .to_string()
    }

    #[test]
    fn an_audio_link_writes_the_linked_prop() {
        let mut state = WebState::new();
        let module = add_bands(&mut state);
        let input = prop_input_id(&state, &module, "u_ringWidth");

        assert!(state.apply_action(&UiAction::new(
            "inputs/createInputLink",
            json!({
                "inputId": input,
                "type": "getter",
                "source": "meyda",
                "location": "meyda/getFeature",
                "args": ["bass"],
                "min": 0.0,
                "max": 1.0,
            })
        )));
        state.set_audio(AudioFrame {
            bass: 1.0,
            ..AudioFrame::default()
        });
        state.evaluate_links();

        assert_ne!(
            state.state().modules[&module].props["u_ringWidth"],
            json!(0.05)
        );
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        assert!(snapshot.input_links.contains_key(&input));
    }

    #[test]
    fn a_tween_link_advances_and_writes_each_frame() {
        let mut state = WebState::new();
        let module = add_bands(&mut state);
        let input = prop_input_id(&state, &module, "u_ringWidth");

        assert!(state.apply_action(&UiAction::new(
            "tweens/createTween",
            json!({
                "id": input,
                "data": [[0.0], [1.0]],
                "duration": 1000.0,
                "direction": "normal",
                "easing": "linear",
                "loop": false,
                "useBpm": false,
                "durationAsTotalTime": true,
            })
        )));
        assert!(state.apply_action(&UiAction::new(
            "inputs/createInputLink",
            json!({
                "inputId": input,
                "type": "state",
                "source": "tween",
                "location": format!("tweens.tweens['{input}'].value[0]"),
                "min": 0.0,
                "max": 1.0,
            })
        )));

        state.evaluate_links();
        let first = state.state().modules[&module].props["u_ringWidth"]
            .as_f64()
            .expect("numeric prop");
        state.evaluate_links();
        let second = state.state().modules[&module].props["u_ringWidth"]
            .as_f64()
            .expect("numeric prop");
        assert!(second > first, "first={first} second={second}");
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        assert!(snapshot.tweens.contains_key(&input));
    }

    #[test]
    fn a_draw_link_reads_the_same_tween_descriptor() {
        let mut state = WebState::new();
        let module = add_bands(&mut state);
        let input = prop_input_id(&state, &module, "u_ringWidth");

        assert!(state.apply_action(&UiAction::new(
            "tweens/createTween",
            json!({
                "id": input,
                "data": [[0.0], [0.5], [1.0]],
                "keyframeTimes": [0.0, 0.5, 1.0],
                "duration": 1000.0,
                "direction": "normal",
                "easing": "linear",
                "loop": false,
                "useBpm": false,
                "durationAsTotalTime": true,
            })
        )));
        assert!(state.apply_action(&UiAction::new(
            "inputs/createInputLink",
            json!({
                "inputId": input,
                "type": "state",
                "source": "draw",
                "location": format!("tweens.tweens['{input}'].value[0]"),
                "min": 0.0,
                "max": 1.0,
            })
        )));
        state.evaluate_links();
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        assert_eq!(snapshot.input_links[&input].source.as_deref(), Some("draw"));
        assert!(snapshot.tweens[&input].keyframe_times.is_some());
    }

    #[test]
    fn the_bundled_isf_corpus_is_registered() {
        let state = WebState::new();
        assert!(
            state.registry().isf_definitions().len() > 300,
            "the bundled ISF corpus must be available in the browser"
        );
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        assert!(
            snapshot
                .registered
                .iter()
                .any(|module| module.name == "Angular"),
            "a bundled ISF shader must appear in the Gallery"
        );
    }

    #[test]
    fn a_bundled_isf_shader_can_be_added() {
        let mut state = WebState::new();
        let id = state
            .add_module("Angular")
            .expect("the bundled ISF corpus knows Angular");
        let module = &state.state().modules[&id];
        assert_eq!(module.meta["type"], json!("isf"));
        assert!(
            !module
                .props
                .as_object()
                .is_some_and(serde_json::Map::is_empty),
            "an ISF shader exposes its input props"
        );
    }

    #[test]
    fn snapshot_reports_the_selected_backend_and_tempo() {
        let mut state = WebState::new();
        state.set_backend("SwiftShader", "webgl2");
        state.set_bpm(126.0);

        let snapshot = state.snapshot([640, 360], 60.0, 16.0);

        assert_eq!(snapshot.health.adapter, "SwiftShader");
        assert_eq!(snapshot.health.backend, "webgl2");
        assert_eq!(snapshot.audio.bpm, 126.0);
    }

    #[test]
    fn two_groups_keep_their_own_module_order() {
        let mut state = WebState::new();
        let first = add_bands(&mut state);

        assert!(state.apply_action(&UiAction::new(
            "groups/createGroup",
            json!({ "name": "Second", "enabled": 1 })
        )));
        let second_group = state.state().groups[1].id.clone();
        let duplicate = state
            .add_module_to_group("Bands", &second_group)
            .expect("add to the second group");

        assert_eq!(state.state().groups[0].modules, vec![first]);
        assert_eq!(state.state().groups[1].modules, vec![duplicate]);
    }

    #[test]
    fn meta_and_group_updates_are_applied() {
        let mut state = WebState::new();
        let id = add_bands(&mut state);

        assert!(state.apply_action(&UiAction::new(
            "modules/updateMeta",
            json!({ "moduleId": id, "metaKey": "enabled", "value": false })
        )));
        assert_eq!(state.state().modules[&id].meta["enabled"], json!(false));

        let group = state.group_id().to_string();
        assert!(state.apply_action(&UiAction::new(
            "groups/UPDATE_GROUP_BY_KEY",
            json!({ "groupId": group, "key": "alpha", "value": 0.5 })
        )));
        assert_eq!(state.state().groups[0].alpha, 0.5);
    }

    fn scene_id(state: &WebState, name: &str) -> String {
        state
            .scene_library()
            .scenes()
            .iter()
            .find(|scene| scene.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|scene| scene.get("id"))
            .and_then(Value::as_str)
            .expect("the default Scene set knows this name")
            .to_string()
    }

    #[test]
    fn the_default_scene_set_is_seeded() {
        let state = WebState::new();
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);

        assert_eq!(snapshot.scenes.len(), 56, "every default Scene is present");
        assert!(
            snapshot
                .scenes
                .iter()
                .any(|scene| scene.name == "Mycelium Bloom"),
            "the seeded Scene list exposes the default names"
        );
        assert_eq!(
            snapshot.performance.min_value, 8.0,
            "the performance timing defaults are exposed to the grid"
        );
        let mycelium = snapshot
            .scenes
            .iter()
            .find(|scene| scene.name == "Mycelium Bloom")
            .expect("Mycelium Bloom");
        assert_eq!(mycelium.cells.len(), 3);
        assert_eq!(mycelium.cells[0].module_names, vec!["Point10 Mycelium"]);
        assert_eq!(mycelium.cells[2].module_names, vec!["Bloom"]);
    }

    #[test]
    fn applying_a_default_scene_replaces_the_live_stack_with_its_links() {
        let mut state = WebState::new();
        let id = scene_id(&state, "Circuits");

        assert!(state.apply_action(&UiAction::new(
            "scenes/applyScene",
            json!({ "sceneId": id })
        )));

        assert_eq!(
            state
                .state()
                .groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Background", "Foreground", "Effects"]
        );
        let module_names: Vec<&str> = state
            .state()
            .modules
            .values()
            .map(|module| module.module_name.as_str())
            .collect();
        assert!(module_names.contains(&"Circuits"), "{module_names:?}");
        assert!(module_names.contains(&"film-grain"), "{module_names:?}");
        assert!(
            state
                .state()
                .input_links
                .values()
                .any(|link| link.source.as_deref() == Some("meyda")),
            "the Scene's explicit bass link is restored"
        );
        assert!(
            !state.tweens.is_empty(),
            "the automatic BPM tween is restored"
        );
        assert_eq!(
            state.group_id(),
            state.state().groups[0].id,
            "the first scene Group becomes the default group"
        );
    }

    #[test]
    fn scene_rename_remove_and_timing_actions_update_the_grid() {
        let mut state = WebState::new();
        let id = scene_id(&state, "Circuits");

        assert!(state.apply_action(&UiAction::new(
            "scenes/updateSceneName",
            json!({ "sceneId": id, "name": "  Circuits Renamed  " })
        )));
        assert!(state.apply_action(&UiAction::new(
            "scenes/updateSceneTiming",
            json!({ "sceneId": id, "minSeconds": 4.0, "maxSeconds": 12.0 })
        )));
        assert!(state.apply_action(&UiAction::new(
            "scenes/setCellEnabled",
            json!({ "sceneId": id, "rowIndex": 2, "enabled": false })
        )));
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        let scene = snapshot
            .scenes
            .iter()
            .find(|scene| scene.id == id)
            .expect("renamed Scene");
        assert_eq!(scene.name, "Circuits Renamed");
        assert_eq!(scene.timing, Some((4.0, 12.0)));
        assert!(scene.cells[2].disabled);

        assert!(state.apply_action(&UiAction::new(
            "scenes/removeScene",
            json!({ "sceneId": id })
        )));
        assert!(
            state
                .snapshot([640, 360], 60.0, 16.0)
                .scenes
                .iter()
                .all(|scene| scene.id != id)
        );
    }

    #[test]
    fn captured_scene_cells_survive_a_snapshot() {
        let mut state = WebState::new();
        assert!(state.add_module("Bands").is_some());
        let id = state.scenes.add_empty(Some("Captured"));

        assert!(state.apply_action(&UiAction::new(
            "scenes/setSceneCellGroup",
            json!({ "sceneId": id, "rowIndex": 1, "groupId": state.group_id() })
        )));
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        let scene = snapshot
            .scenes
            .iter()
            .find(|scene| scene.id == id)
            .expect("captured Scene");
        assert!(!scene.cells[1].is_empty());
    }

    #[test]
    fn the_live_stack_and_scenes_round_trip_through_persistence() {
        let mut state = WebState::new();
        let id = scene_id(&state, "Circuits");
        assert!(state.apply_action(&UiAction::new(
            "scenes/applyScene",
            json!({ "sceneId": id })
        )));
        assert!(state.add_module("Bands").is_some());
        let module_ids: Vec<String> = state.state().modules.keys().cloned().collect();
        let scene_names: Vec<String> = state
            .scene_library()
            .scenes()
            .iter()
            .filter_map(|scene| scene.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect();

        let document = state.to_persisted_json();
        let mut restored = WebState::new();
        assert!(restored.restore_persisted_json(&document));

        assert_eq!(
            restored.state().modules.keys().collect::<Vec<_>>(),
            module_ids.iter().collect::<Vec<_>>(),
            "active modules and their order survive a refresh"
        );
        assert_eq!(
            restored
                .scene_library()
                .scenes()
                .iter()
                .filter_map(|scene| scene.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>(),
            scene_names,
            "edits to the Scene list survive a refresh"
        );
        assert_eq!(restored.group_id(), state.group_id());
        assert!(!restored.is_dirty(), "a restore is not a new edit");
    }

    #[test]
    fn persistence_rejects_incompatible_documents() {
        let mut state = WebState::new();
        let before = state.state().modules.len();

        assert!(!state.restore_persisted_json(&json!({
            "version": 999,
            "engine": {},
        })));
        assert_eq!(state.state().modules.len(), before);
    }

    fn enable_performance(state: &mut WebState) {
        assert!(state.apply_action(&UiAction::new(
            "scenes/updatePerformance",
            json!({ "config": { "minSeconds": 1.0, "maxSeconds": 2.0 } })
        )));
        assert!(state.apply_action(&UiAction::new(
            "scenes/setPerformanceEnabled",
            json!({ "enabled": true })
        )));
    }

    #[test]
    fn performance_mode_populates_rows_and_advances_them() {
        let mut state = WebState::new();
        enable_performance(&mut state);
        let first = state.snapshot([640, 360], 60.0, 16.0);
        assert!(
            !first.active_scene_rows.is_empty(),
            "starting performance applies a Scene per non-empty row"
        );
        assert!(first.performance.enabled);
        assert!(first.applied_scene_id.is_some());
        let before = first.active_scene_rows.clone();

        for _ in 0..25 {
            state.tick_performance(0.1, false);
        }
        let after = state.snapshot([640, 360], 60.0, 16.0);
        assert_ne!(
            after.active_scene_rows, before,
            "rows advance once their dwell deadline passes"
        );
        assert_ne!(
            after
                .modules
                .iter()
                .map(|m| &m.module_name)
                .collect::<Vec<_>>(),
            first
                .modules
                .iter()
                .map(|m| &m.module_name)
                .collect::<Vec<_>>(),
            "the live modules follow the advanced rows"
        );
    }

    #[test]
    fn stopping_performance_clears_the_live_rows() {
        let mut state = WebState::new();
        enable_performance(&mut state);
        assert!(
            !state
                .snapshot([640, 360], 60.0, 16.0)
                .active_scene_rows
                .is_empty()
        );

        assert!(state.apply_action(&UiAction::new(
            "scenes/setPerformanceEnabled",
            json!({ "enabled": false })
        )));
        assert!(
            state
                .snapshot([640, 360], 60.0, 16.0)
                .active_scene_rows
                .is_empty()
        );
    }

    #[test]
    fn a_manual_scene_apply_marks_every_covered_row_live() {
        let mut state = WebState::new();
        enable_performance(&mut state);
        let id = scene_id(&state, "Circuits");
        assert!(state.apply_action(&UiAction::new(
            "scenes/applyScene",
            json!({ "sceneId": id })
        )));
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        assert_eq!(snapshot.applied_scene_id.as_deref(), Some(id.as_str()));
        assert_eq!(
            snapshot.active_scene_rows.get(&0).map(String::as_str),
            Some(id.as_str())
        );
    }

    fn tick_transition_seconds(state: &mut WebState, seconds: f64) {
        for _ in 0..(seconds / 0.1).ceil() as usize {
            state.tick_transitions(0.1);
        }
    }

    fn apply_named(state: &mut WebState, name: &str) {
        let id = scene_id(state, name);
        assert!(state.apply_action(&UiAction::new(
            "scenes/applyScene",
            json!({ "sceneId": id })
        )));
    }

    #[test]
    fn whole_scene_changes_crossfade_and_clean_up_the_ghosts() {
        let mut state = WebState::new();
        apply_named(&mut state, "Mycelium Bloom");
        apply_named(&mut state, "Circuits");

        let transition = state.transition.as_ref().expect("a whole-Scene fade");
        assert!(transition.effect.is_none(), "the default is the Fade path");
        assert!(
            state
                .state()
                .groups
                .iter()
                .any(|group| group.name == "Background" && group.alpha == 0.0),
            "incoming Groups start transparent"
        );
        assert!(
            state
                .state()
                .groups
                .iter()
                .any(|group| group.name == "Background" && group.alpha > 0.0),
            "outgoing Groups stay visible"
        );

        tick_transition_seconds(&mut state, 1.0);
        let halfway = state
            .state()
            .groups
            .iter()
            .filter(|group| group.name == "Background")
            .map(|group| group.alpha)
            .collect::<Vec<_>>();
        assert!(
            halfway.iter().any(|alpha| *alpha > 0.0 && *alpha < 1.0),
            "the fade is in flight: {halfway:?}"
        );

        tick_transition_seconds(&mut state, 1.5);
        assert!(state.transition.is_none(), "the fade completes");
        let old = state
            .state()
            .groups
            .iter()
            .filter(|group| group.name == "Background")
            .count();
        assert_eq!(old, 1, "only the incoming Background survives");
    }

    #[test]
    fn shader_transitions_are_selected_from_allowed_transitions() {
        let mut state = WebState::new();
        apply_named(&mut state, "Mycelium Bloom");
        assert!(state.apply_action(&UiAction::new(
            "scenes/updatePerformance",
            json!({ "config": { "allowedTransitions": ["Three Crossfade"] } })
        )));
        apply_named(&mut state, "Circuits");

        let info = state
            .scene_transition_info()
            .expect("a shader transition is active");
        assert!(matches!(info.effect, TransitionEffect::Crossfade));
        assert!(
            state.take_pending_transition_capture().is_some(),
            "the app is told to capture the outgoing frame"
        );
        assert_eq!(info.from_groups.len(), 3);
        tick_transition_seconds(&mut state, 3.0);
        assert!(state.transition.is_none());
    }

    #[test]
    fn none_transitions_switch_immediately() {
        let mut state = WebState::new();
        apply_named(&mut state, "Mycelium Bloom");
        assert!(state.apply_action(&UiAction::new(
            "scenes/updatePerformance",
            json!({ "config": { "allowedTransitions": ["None"] } })
        )));
        apply_named(&mut state, "Circuits");
        assert!(state.transition.is_none());
        assert!(state.row_transitions.is_empty());
    }

    #[test]
    fn performance_row_advances_leave_a_crossfade_ghost() {
        let mut state = WebState::new();
        enable_performance(&mut state);
        for _ in 0..25 {
            state.tick_performance(0.1, false);
        }
        assert!(
            !state.row_transitions.is_empty(),
            "an advance starts a row crossfade"
        );
        let fades = state.row_fade_infos();
        assert!(!fades.is_empty());
        assert_eq!(fades[0].progress, 0.0);
        assert!(
            state
                .state()
                .groups
                .iter()
                .any(|group| group.transition_ghost == Some(true)),
            "the outgoing row Group is kept as a ghost"
        );
        let visible = state.snapshot([640, 360], 60.0, 16.0);
        assert!(
            visible
                .groups
                .iter()
                .all(|group| group.name != "Gallery" && !group.id.is_empty()),
            "the snapshot is still coherent while a ghost exists"
        );
        tick_transition_seconds(&mut state, 3.0);
        assert!(state.row_transitions.is_empty());
        assert!(
            state
                .state()
                .groups
                .iter()
                .all(|group| group.transition_ghost != Some(true)),
            "the ghost is removed after the fade"
        );
    }

    #[test]
    fn switching_the_timing_unit_converts_seconds_and_overrides() {
        let mut state = WebState::new();
        let id = scene_id(&state, "Circuits");
        assert!(state.apply_action(&UiAction::new(
            "scenes/updateSceneTiming",
            json!({ "sceneId": id, "minSeconds": 4.0, "maxSeconds": 12.0 })
        )));
        state.set_bpm(120.0);
        assert!(state.apply_action(&UiAction::new(
            "scenes/setTimingUnit",
            json!({ "unit": "bars" })
        )));

        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        assert_eq!(snapshot.performance.timing_unit, modv_ui::TimingUnit::Bars);
        assert_eq!(snapshot.performance.min_value, 4.0);
        assert_eq!(snapshot.performance.max_value, 12.0);
        let scene = snapshot
            .scenes
            .iter()
            .find(|scene| scene.id == id)
            .expect("Scene");
        assert_eq!(scene.timing, Some((2.0, 6.0)));
    }

    #[test]
    fn copy_cell_carries_its_inputs_and_clear_removes_its_modules() {
        let mut state = WebState::new();
        let source = scene_id(&state, "Circuits");
        let target = state.scenes.add_empty(Some("Copied cell"));
        assert!(state.scenes.copy_cell(&source, 0, &target, 1));

        let copied = state.scenes.find(&target).expect("copied Scene").clone();
        let modules = copied
            .get("modules")
            .and_then(Value::as_object)
            .expect("modules");
        assert!(!modules.is_empty());
        let inputs = copied
            .pointer("/inputs/inputs")
            .and_then(Value::as_object)
            .expect("inputs");
        assert!(
            !inputs.is_empty(),
            "the copied Group brings its control inputs"
        );
        let links = copied
            .pointer("/inputs/inputLinks")
            .and_then(Value::as_object)
            .expect("links");
        assert!(!links.is_empty(), "the copied module keeps its links");

        let module_id = modules.keys().next().expect("module id").clone();
        assert!(state.scenes.clear_cell(&target, 1));
        let cleared = state.scenes.find(&target).expect("cleared Scene").clone();
        assert!(
            cleared
                .get("modules")
                .and_then(Value::as_object)
                .is_none_or(|modules| !modules.contains_key(&module_id)),
            "clearing the cell removes its orphaned module document"
        );
    }

    #[test]
    fn phrase_flips_can_advance_a_row_before_its_deadline() {
        let mut state = WebState::new();
        enable_performance(&mut state);
        for row in &mut state.performance_runtime.rows {
            row.dwell = 2.0;
            row.target = 100.0;
        }
        let before = state.performance_runtime.active.clone();
        state.tick_performance(0.1, true);
        assert_ne!(
            state.performance_runtime.active, before,
            "a phrase flip advances once the musical chance reaches 1"
        );
    }

    #[test]
    fn stopping_performance_finishes_in_flight_row_fades() {
        let mut state = WebState::new();
        enable_performance(&mut state);
        for _ in 0..25 {
            state.tick_performance(0.1, false);
        }
        assert!(!state.row_transitions.is_empty());
        assert!(state.apply_action(&UiAction::new(
            "scenes/setPerformanceEnabled",
            json!({ "enabled": false })
        )));
        assert!(state.row_transitions.is_empty());
        assert!(
            state
                .state()
                .groups
                .iter()
                .all(|group| group.transition_ghost != Some(true))
        );
    }

    #[test]
    fn group_inherit_composition_round_trips_through_the_snapshot() {
        let mut state = WebState::new();
        let group = state.group_id().to_string();

        assert!(state.apply_action(&UiAction::new(
            "groups/UPDATE_GROUP_BY_KEY",
            json!({
                "groupId": group,
                "key": "inheritFrom",
                "value": modv_engine::engine_state::INHERIT_COMPOSITION,
            })
        )));
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        assert_eq!(snapshot.groups[0].inherit_from, InheritTarget::Composition);

        assert!(state.apply_action(&UiAction::new(
            "groups/UPDATE_GROUP_BY_KEY",
            json!({ "groupId": group, "key": "inheritFrom", "value": -1 })
        )));
        let snapshot = state.snapshot([640, 360], 60.0, 16.0);
        assert_eq!(snapshot.groups[0].inherit_from, InheritTarget::Previous);
    }
}
