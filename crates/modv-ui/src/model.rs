//! Read models the native UI renders.
//!
//! `modv-app` builds one [`EngineSnapshot`] per frame from the authoritative
//! `Store`. The UI only reads this snapshot and emits actions; it never holds
//! a reference to the store, so egui's immediate-mode draw calls cannot
//! conflict with a dispatch.

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
    /// Enum options, when the prop declares them.
    pub options: Vec<String>,
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
