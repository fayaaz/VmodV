//! Read models the native UI renders.
//!
//! `modv-app` builds one [`EngineSnapshot`] per frame from the authoritative
//! `Store`. The UI only reads this snapshot and emits actions; it never holds
//! a reference to the store, so egui's immediate-mode draw calls cannot
//! conflict with a dispatch.

use std::collections::HashMap;

use serde_json::Value;

/// Everything the shell needs to draw one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineSnapshot {
    /// Groups in store order (the render order is derived by the engine).
    pub groups: Vec<GroupView>,
    /// Active module instances.
    pub modules: Vec<ModuleView>,
    /// Saved Scenes in grid order.
    pub scenes: Vec<SceneView>,
    /// Every registered module definition (Gallery/Gallery search).
    pub registered: Vec<ModuleMetaView>,
    /// Performance-mode configuration and runtime state.
    pub performance: PerformanceView,
    /// Audio analysis for meters.
    pub audio: AudioView,
    /// Output resolution (the compositor's main texture size).
    pub resolution: [u32; 2],
    /// Measured main-loop frames per second.
    pub fps: f32,
    /// Most recent frame time in milliseconds.
    pub frame_ms: f32,
    /// Performance row names, in grid order.
    pub scene_rows: Vec<String>,
    /// The Scene the engine last applied.
    pub applied_scene_id: Option<String>,
    /// Engine/adapter health for the status bar.
    pub health: EngineHealth,
    /// Gallery hover previews keyed by module name.
    pub gallery_previews: HashMap<String, PreviewTexture>,
    /// Scene-cell hover previews keyed by [`scene_preview_key`].
    pub scene_previews: HashMap<String, PreviewTexture>,
    /// Live texture sources the Inspector's texture picker can bind.
    pub texture_sources: Vec<TextureSourceView>,
    /// Input links keyed by the input id they write to.
    pub input_links: HashMap<String, InputLinkView>,
}

/// One uploaded preview texture (an egui texture owned by the shell).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewTexture {
    pub id: egui::TextureId,
    pub width: u32,
    pub height: u32,
}

/// The key used by [`EngineSnapshot::scene_previews`].
pub fn scene_preview_key(scene_id: &str, row: usize) -> String {
    format!("scene:{scene_id}:{row}")
}

/// The key used by [`EngineSnapshot::gallery_previews`].
pub fn gallery_preview_key(module_name: &str) -> String {
    format!("module:{module_name}")
}

/// One engine Input Link (audio getter, state path, MIDI or OSC).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputLinkView {
    pub input_id: String,
    pub kind: String,
    pub location: Option<String>,
    pub args: Option<Value>,
    pub source: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub matcher: Option<Value>,
}

/// One live texture source (`group:<id>`, `isf:<id>`, `camera:<id>`, ...).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextureSourceView {
    pub id: String,
    pub label: String,
    /// `group`, `isf`, `two-d`, `point10`, `camera`, `video` or `screen`.
    pub kind: String,
}

/// A hover-preview request emitted by the UI and answered by the shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewRequest {
    Module {
        module_name: String,
        /// Composite the preview over the live output (hover previews).
        with_output: bool,
    },
    Scene {
        scene_id: String,
        row_index: usize,
    },
}

impl EngineSnapshot {
    /// The modules belonging to `group_id`, in group order.
    pub fn modules_in_group(&self, group_id: &str) -> Vec<&ModuleView> {
        let Some(group) = self.groups.iter().find(|group| group.id == group_id) else {
            return Vec::new();
        };
        group
            .module_ids
            .iter()
            .filter_map(|id| self.modules.iter().find(|module| &module.id == id))
            .collect()
    }

    /// The module with `module_id`.
    pub fn module(&self, module_id: &str) -> Option<&ModuleView> {
        self.modules.iter().find(|module| module.id == module_id)
    }

    /// The Scene with `scene_id`.
    pub fn scene(&self, scene_id: &str) -> Option<&SceneView> {
        self.scenes.iter().find(|scene| scene.id == scene_id)
    }

    /// The module definition with `name`.
    pub fn registered_module(&self, name: &str) -> Option<&ModuleMetaView> {
        self.registered.iter().find(|module| module.name == name)
    }
}

/// One Group (a render-graph row).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub hidden: bool,
    pub alpha: f32,
    pub composite_operation: String,
    pub inherit: bool,
    pub clearing: bool,
    pub pipeline: bool,
    /// Module ids in group draw order.
    pub module_ids: Vec<String>,
}

impl GroupView {
    pub fn module_count(&self) -> usize {
        self.module_ids.len()
    }
}

/// One active Module instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModuleView {
    pub id: String,
    /// Registry key, e.g. `Bands`.
    pub module_name: String,
    /// Owning Group id (`""` when ungrouped).
    pub group_id: String,
    pub enabled: bool,
    pub alpha: f32,
    pub module_type: String,
    pub props: Vec<PropView>,
}

/// A single editable property of an active Module.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropView {
    pub name: String,
    /// Engine prop type token: `float`, `int`, `bool`, `text`, `enum`, `event`,
    /// `color`, `vec2`/`vec3`/`vec4`, `texture`, `palette`, `tween`, `font`.
    pub prop_type: String,
    pub value: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    /// Registered default value (the Inspector's reset target).
    pub default: Option<Value>,
    /// The prop's input id (`$props[prop].id`), when one exists.
    pub input_id: Option<String>,
    /// The tween descriptor linked to this prop, when one exists.
    pub tween: Option<TweenView>,
    /// Enum options, when the prop declares them.
    pub options: Vec<String>,
}

/// A tween descriptor read from the engine state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TweenView {
    pub id: String,
    pub duration: f64,
    pub direction: String,
    pub easing: String,
    pub looping: bool,
    pub use_bpm: bool,
    pub bpm_division: f64,
    pub steps: f64,
    pub frames: u64,
}

/// One saved Scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Per-Scene timing override in seconds, when set.
    pub timing: Option<f64>,
    /// Cells by performance row. A `null` Scene row is represented by an
    /// empty cell (`module_names` empty).
    pub cells: Vec<SceneCellView>,
}

impl SceneView {
    /// The cell at `row`, if the Scene defines one.
    pub fn cell(&self, row: usize) -> Option<&SceneCellView> {
        self.cells.get(row)
    }
}

/// One Scene-grid cell (a Group snapshot).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneCellView {
    pub row: usize,
    pub module_names: Vec<String>,
}

impl SceneCellView {
    pub fn is_empty(&self) -> bool {
        self.module_names.is_empty()
    }

    pub fn label(&self) -> String {
        match self.module_names.as_slice() {
            [] => "Empty".to_string(),
            [one] => one.clone(),
            [first, rest @ ..] => format!("{first} +{}", rest.len()),
        }
    }
}

/// One registered module definition (Gallery metadata).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModuleMetaView {
    pub name: String,
    pub module_type: String,
    pub category: String,
    pub author: String,
    pub version: String,
    pub description: String,
}

impl ModuleMetaView {
    /// The Gallery section this module belongs to (`effects`, `2d-scenes`,
    /// `3d-scenes`), following `Gallery.vue`'s category fallback.
    pub fn gallery_category(&self) -> &'static str {
        match self.category.as_str() {
            "effects" => "effects",
            "2d-scenes" => "2d-scenes",
            "3d-scenes" => "3d-scenes",
            _ => match self.module_type.as_str() {
                "2d" => "2d-scenes",
                "3d" => "3d-scenes",
                _ => "effects",
            },
        }
    }
}

/// Performance-mode configuration plus the runtime Scene id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PerformanceView {
    pub enabled: bool,
    pub current_scene_id: Option<String>,
    pub row_names: Vec<String>,
    pub min_seconds: f64,
    pub max_seconds: f64,
    pub transition_seconds: f64,
    pub allowed_transitions: Vec<String>,
}

/// Audio analysis values shown by the meters and BPM readout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioView {
    pub bpm: f64,
    pub kick: bool,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
}

/// Status-bar health information.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineHealth {
    pub adapter: String,
    pub backend: String,
    pub last_error: Option<String>,
    pub frames_presented: u64,
    pub output_open: bool,
    pub audio_active: bool,
}
