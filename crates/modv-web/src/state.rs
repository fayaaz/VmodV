//! Browser-owned engine state.
//!
//! The desktop shell's `Store` (presets, Scenes, filesystem, NDI, OSC, MIDI,
//! plugins) is deliberately not part of the web build. This module owns the
//! subset the browser shell needs: live Groups, their active modules, prop
//! editing, and the `EngineSnapshot` the existing egui shell renders.
//!
//! The state is pure (no wgpu, no DOM), so it is unit-tested on the host.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::{Arc, OnceLock};

use include_dir::{Dir, include_dir};
use serde_json::{Map, Value, json};

use modv_app::input_links::{BeatsState, LinkTarget, LinkWrite, evaluate_input_links};
use modv_app::module_registry::ModuleRegistry;
use modv_engine::engine_state::{
    EngineState, GROUP_DISABLED, GROUP_ENABLED, Group, InheritFrom, ModuleInstance,
};
use modv_engine::inputs::{Input, InputLink};
use modv_engine::props::coerce_prop_value;
use modv_engine::tweens::{TweenDescriptor, TweenSet};
use modv_renderers::two_d::AudioFrame;
use modv_ui::model::{
    AudioView, EngineHealth, EngineSnapshot, GroupView, InheritTarget, InputLinkView,
    ModuleMetaView, ModuleView, PerformanceView, PropEnumMember, PropView, TweenView,
};
use modv_ui::state::UiAction;

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
        let mut this = Self {
            registry: bundled_registry(),
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
        };
        this.install_group_inputs("web-group-1");
        this
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
        Some(id)
    }

    /// Remove an active module from every Group, with its links and tweens.
    pub fn remove_module(&mut self, module_id: &str) -> bool {
        let removed = self.state.modules.shift_remove(module_id).is_some();
        if !removed {
            return false;
        }
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
    pub fn apply_action(&mut self, action: &UiAction) -> bool {
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
            // Selection-only, Scene, media, audio-device, NDI/OSC/MIDI and
            // plugin actions are outside the web shell's cut line.
            _ => false,
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
                    Value::String(id) => InheritFrom::Group(id.clone()),
                    Value::Number(number) => InheritFrom::Previous(number.as_i64().unwrap_or(-1)),
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
        let groups = self.state.groups.iter().map(group_view).collect();
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
        let modules = self
            .state
            .modules
            .values()
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
        let performance = PerformanceView {
            enabled: false,
            current_scene_id: None,
            row_names: Vec::new(),
            ..PerformanceView::default()
        };
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
}
