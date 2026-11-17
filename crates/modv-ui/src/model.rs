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
    /// Live row monitors keyed by performance row (the row gutter's
    /// thumbnail of the row's currently active Group).
    pub row_monitors: HashMap<usize, RowMonitorView>,
    /// Live texture sources the Inspector's texture picker can bind.
    pub texture_sources: Vec<TextureSourceView>,
    /// Input links keyed by the input id they write to.
    pub input_links: HashMap<String, InputLinkView>,
    /// Tween descriptors keyed by their id (which is the input id they drive).
    pub tweens: HashMap<String, TweenView>,
    /// Expression assignments in store order (`expressions.js` state shape).
    pub expressions: Vec<ExpressionView>,
    /// Live capture state for the Media panel.
    pub media: MediaSnapshot,
    /// Native MIDI input state.
    pub midi: MidiSnapshot,
    /// Enumerated cpal input devices for the Audio panel.
    pub audio_devices: Vec<AudioDeviceView>,
    /// The live native capture selection (the Audio panel's active choice).
    pub audio_selection: AudioSelectionView,
}

/// One enumerable audio input device (a cpal input from `modv-media`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioDeviceView {
    /// Stable cpal device id (`host:device`), the value
    /// [`AudioSelectionView::device`] stores.
    pub id: String,
    /// Human-readable device name.
    pub name: String,
    /// Default input channel count.
    pub channels: u16,
    /// Default input sample rate in Hz.
    pub default_sample_rate: u32,
}

impl AudioDeviceView {
    /// The compact secondary line under a picker row (`2 ch · 48 kHz`).
    pub fn detail(&self) -> String {
        format!(
            "{} ch · {}",
            self.channels,
            sample_rate_label(self.default_sample_rate)
        )
    }
}

/// The native shell's live audio capture selection.
///
/// `source` mirrors `AudioSourceSelection::as_str` (`pipewire` or `input`).
/// `input` with no `device` means the host default input device, exactly like
/// `--audio-source input` without a device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSelectionView {
    pub source: String,
    pub device: Option<String>,
    /// The last live-switch failure. The previous feed keeps running when this
    /// is set, so the panel can say so instead of implying silence.
    pub error: Option<String>,
}

impl Default for AudioSelectionView {
    fn default() -> Self {
        Self {
            source: "pipewire".to_string(),
            device: None,
            error: None,
        }
    }
}

impl AudioSelectionView {
    pub fn is_pipewire(&self) -> bool {
        self.source == "pipewire"
    }

    pub fn is_default_input(&self) -> bool {
        self.source == "input" && self.device.is_none()
    }

    /// Whether `device` is the selected input (matching its id or name, like
    /// `--audio-device` accepts both).
    pub fn matches_device(&self, device: &AudioDeviceView) -> bool {
        let Some(selected) = self.device.as_deref() else {
            return false;
        };
        self.source == "input"
            && (selected.eq_ignore_ascii_case(&device.id)
                || selected.eq_ignore_ascii_case(&device.name))
    }

    /// The selected input device, when it is still present in the list.
    pub fn active_device<'a>(&self, devices: &'a [AudioDeviceView]) -> Option<&'a AudioDeviceView> {
        if self.source != "input" {
            return None;
        }
        devices.iter().find(|device| self.matches_device(device))
    }

    /// The friendly name the Audio panel's "in use" banner shows.
    ///
    /// Device rows resolve to their human-readable name; the system sources
    /// get an explicit label instead of exposing a cpal id.
    pub fn active_label(&self, devices: &[AudioDeviceView]) -> String {
        if let Some(device) = self.active_device(devices) {
            return device.name.clone();
        }
        match (self.source.as_str(), self.device.as_deref()) {
            ("pipewire", _) => "System audio".to_string(),
            ("input", None) => "System default input".to_string(),
            ("input", Some(_)) => "Input device".to_string(),
            (source, _) => source.to_string(),
        }
    }

    /// The one-line explanation under the banner's friendly name.
    pub fn active_subtitle(&self, devices: &[AudioDeviceView]) -> String {
        if self.active_device(devices).is_some() {
            return "Microphone or audio interface".to_string();
        }
        match (self.source.as_str(), self.device.as_deref()) {
            ("pipewire", _) => "Captures what your speakers play".to_string(),
            ("input", None) => "Follows the system's default device".to_string(),
            ("input", Some(_)) => "The selected device is not currently connected".to_string(),
            _ => String::new(),
        }
    }

    /// The banner's technical line: channel count, sample rate and the stable
    /// source id (the id stays out of the primary label).
    pub fn active_detail(&self, devices: &[AudioDeviceView]) -> String {
        if let Some(device) = self.active_device(devices) {
            return format!(
                "{} ch · {} · {}",
                device.channels,
                sample_rate_label(device.default_sample_rate),
                device.id
            );
        }
        match (self.source.as_str(), self.device.as_deref()) {
            ("pipewire", _) => "PipeWire monitor · pipewire".to_string(),
            ("input", None) => "System default · input".to_string(),
            ("input", Some(device)) => format!("input · {device}"),
            (source, _) => source.to_string(),
        }
    }

    /// A human-readable label for the active source (device names are
    /// resolved by the panel from [`EngineSnapshot::audio_devices`]).
    pub fn describe(&self) -> String {
        match (self.source.as_str(), self.device.as_deref()) {
            ("pipewire", _) => "System audio (PipeWire monitor)".to_string(),
            ("input", Some(device)) => format!("input: {device}"),
            ("input", None) => "Default input (system default)".to_string(),
            (source, _) => source.to_string(),
        }
    }
}

/// `48000` → `48 kHz`, `44100` → `44.1 kHz`, `0` → `unknown rate`.
fn sample_rate_label(hz: u32) -> String {
    if hz == 0 {
        return "unknown rate".to_string();
    }
    if hz < 1_000 {
        return format!("{hz} Hz");
    }
    let khz = f64::from(hz) / 1_000.0;
    if khz.fract().abs() < f64::EPSILON {
        format!("{khz:.0} kHz")
    } else {
        format!("{khz:.1} kHz")
    }
}

/// One uploaded preview texture (an egui texture owned by the shell).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewTexture {
    pub id: egui::TextureId,
    pub width: u32,
    pub height: u32,
}

/// The live layer state of one performance row (the grid's row monitor).
///
/// The shell keeps a slow-cadence GPU snapshot of the row's live Group and
/// registers it with `egui-wgpu` as a native texture; the grid letterboxes
/// it into the row gutter, dimmed while performance mode is off. `texture`
/// is `None` from the moment the Group appears until the row's first
/// monitored refresh lands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowMonitorView {
    /// The live Group rendering this row.
    pub group_id: String,
    /// The uploaded monitor snapshot, once the first refresh lands.
    pub texture: Option<PreviewTexture>,
}

/// The key used by [`EngineSnapshot::scene_previews`].
pub fn scene_preview_key(scene_id: &str, row: usize) -> String {
    format!("scene:{scene_id}:{row}")
}

/// The key used by [`EngineSnapshot::gallery_previews`].
pub fn gallery_preview_key(module_name: &str) -> String {
    format!("module:{module_name}")
}

/// Undo premultiplication on a tightly packed RGBA8 pixel buffer.
///
/// The compositor's read-backs (`Compositor::read_main_rgba8` and the
/// isolated Gallery/Scene preview renderers) produce **premultiplied**
/// RGBA — every RGB sample already scaled by its alpha. egui CPU textures
/// are created from straight alpha (`ColorImage::from_rgba_unmultiplied`
/// premultiplies on upload), so the preview upload path converts first:
/// shipping premultiplied bytes as straight alpha (or flattening alpha to
/// opaque, the old behaviour) renders transparent halos as dark fringes.
///
/// Opaque pixels are unchanged and a zero-alpha pixel stays zero (the
/// premultiplied contract already makes it `(0,0,0,0)`). Conversion is
/// round-to-nearest and clamped, so a malformed sample above its alpha
/// cannot wrap.
pub fn un_premultiply_rgba(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = pixel[3];
        if alpha == 0 || alpha == 255 {
            continue;
        }
        let alpha = u32::from(alpha);
        for sample in pixel.iter_mut().take(3) {
            let value = u32::from(*sample);
            *sample = ((value * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

/// The native MediaHub's camera and running-source state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaSnapshot {
    pub available: bool,
    pub error: Option<String>,
    pub cameras: Vec<CameraView>,
    pub sources: Vec<MediaSourceView>,
}

/// One enumerable camera.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CameraView {
    pub id: String,
    pub name: String,
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u32,
}

/// One running media source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaSourceView {
    pub source_id: String,
    pub kind: String,
    pub index: usize,
    pub state: String,
    pub width: u32,
    pub height: u32,
    pub frames: u64,
    pub dropped: u64,
    pub last_error: Option<String>,
}

/// Native MIDI availability and devices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MidiSnapshot {
    pub available: bool,
    pub error: Option<String>,
    pub devices: Vec<MidiDeviceView>,
}

/// One MIDI input and its engine flags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MidiDeviceView {
    /// The engine state key (`${id}-${name}-${manufacturer}`).
    pub key: String,
    pub name: String,
    pub connected: bool,
    pub listen_for_input: bool,
    pub listen_for_clock: bool,
    pub cc_latch: bool,
    pub note_on_latch: bool,
}

/// A MediaHub operation requested by the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaAction {
    StartCamera { id: String },
    StartTestPattern,
    Stop { source_id: String },
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

/// One expression assignment (`expressions.js`: `{ id, inputId, expression }`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExpressionView {
    pub id: String,
    pub input_id: String,
    pub expression: String,
    pub evaluate_every_frame: bool,
}

/// The blend/composite operations `ActiveModule.vue` offers
/// (`util/composite-operations.js`).
pub const COMPOSITE_OPERATIONS: &[&str] = &[
    "normal",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "color-dodge",
    "color-burn",
    "hard-light",
    "soft-light",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
    "clear",
    "copy",
    "destination",
    "source-over",
    "destination-over",
    "source-in",
    "destination-in",
    "source-out",
    "destination-out",
    "source-atop",
    "destination-atop",
    "xor",
    "lighter",
];

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

    /// The expression assignment for `input_id`, when one exists
    /// (`expressions/getByInputId`).
    pub fn expression_for_input(&self, input_id: &str) -> Option<&ExpressionView> {
        self.expressions
            .iter()
            .find(|assignment| assignment.input_id == input_id)
    }

    /// The tween descriptor linked to `input_id`, when one exists.
    pub fn tween_for_input(&self, input_id: &str) -> Option<&TweenView> {
        self.tweens.get(input_id)
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
    /// Engine `alphaInputId`; a link row is drawn when present.
    pub alpha_input_id: Option<String>,
    pub composite_operation: String,
    /// Engine `compositeOperationInputId`; a link row is drawn when present.
    pub composite_operation_input_id: Option<String>,
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
    /// Engine `meta.alphaInputId`; a link row is drawn when present.
    pub alpha_input_id: Option<String>,
    /// Engine `meta.compositeOperation` (Inspector Blend control).
    pub composite_operation: String,
    /// Engine `meta.compositeOperationInputId`; a link row is drawn when present.
    pub composite_operation_input_id: Option<String>,
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
    /// The prop's component sub-inputs (`${parent}-${key}`, the JS Vec/Color
    /// sliders' focused inputs), keyed `r/g/b/a` or `0..N`, when the engine
    /// minted them.
    pub components: Vec<PropComponentView>,
    /// The tween descriptor linked to this prop, when one exists.
    pub tween: Option<TweenView>,
    /// Enum options, when the prop declares them.
    pub options: Vec<String>,
}

/// One component sub-input of a prop (a Vec lane or Color channel).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PropComponentView {
    /// The component key: `r`/`g`/`b`/`a` for colors, `0`..`N` for vecs.
    pub key: String,
    /// Sub-input id (`${parent input id}-${key}`).
    pub input_id: String,
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
    /// Disabled in performance mode only; a manual Scene/cell apply still
    /// renders it. Persisted in `scenes.json` as `performance.disabledCells`.
    pub disabled: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `round(a * alpha / 255)`, the premultiplication the compositor applies.
    fn premultiply(straight: [u8; 4]) -> [u8; 4] {
        let alpha = u32::from(straight[3]);
        let mut out = straight;
        for channel in 0..3 {
            let value = u32::from(straight[channel]);
            out[channel] = ((value * alpha + 127) / 255).min(255) as u8;
        }
        out
    }

    #[test]
    fn un_premultiply_restores_rgb_scaled_by_alpha() {
        // Straight (200, 100, 50) at alpha 51 premultiplies exactly to
        // (40, 20, 10): 200 * 51 / 255 == 40, no rounding on either leg.
        let mut pixels = [40, 20, 10, 51];
        un_premultiply_rgba(&mut pixels);
        assert_eq!(pixels, [200, 100, 50, 51]);
    }

    #[test]
    fn un_premultiply_keeps_opaque_pixels_unchanged() {
        let mut pixels = [200, 100, 50, 255];
        un_premultiply_rgba(&mut pixels);
        assert_eq!(pixels, [200, 100, 50, 255], "alpha 255 needs no scaling");
    }

    #[test]
    fn un_premultiply_keeps_zero_alpha_pixels_zero() {
        let mut pixels = [0, 0, 0, 0];
        un_premultiply_rgba(&mut pixels);
        assert_eq!(pixels, [0, 0, 0, 0]);
    }

    #[test]
    fn un_premultiply_round_trips_a_premultiplied_preview() {
        // The pixel-level regression the preview upload path needs: RGB
        // scaled by alpha before the conversion is restored after it. Both
        // legs round to nearest, so an arbitrary value can drift by ±2; the
        // first triple is chosen to be exact on both legs.
        for straight in [
            [200, 100, 50, 51],
            [255, 128, 0, 51],
            [1, 2, 3, 51],
            [17, 200, 99, 51],
        ] {
            let mut round = premultiply(straight);
            un_premultiply_rgba(&mut round);
            for channel in 0..4 {
                assert!(
                    (i16::from(round[channel]) - i16::from(straight[channel])).abs() <= 2,
                    "channel {channel}: {round:?} did not restore {straight:?}"
                );
            }
        }

        // Exact round trip for the well-behaved triple: 200·51/255 == 40.
        let mut round = premultiply([200, 100, 50, 51]);
        un_premultiply_rgba(&mut round);
        assert_eq!(round, [200, 100, 50, 51]);
    }

    #[test]
    fn un_premultiply_rounds_to_nearest_and_clamps() {
        // alpha 3: (1 * 255 + 1) / 3 == 85 (rounded to nearest).
        let mut pixels = [1, 1, 1, 3];
        un_premultiply_rgba(&mut pixels);
        assert_eq!(pixels, [85, 85, 85, 3]);

        // A malformed sample above its alpha clamps to 255 instead of
        // wrapping — premultiplied input can never do this, but the helper
        // must stay total.
        let mut malformed = [200, 200, 200, 1];
        un_premultiply_rgba(&mut malformed);
        assert_eq!(malformed, [255, 255, 255, 1]);
    }

    fn device(id: &str, name: &str) -> AudioDeviceView {
        AudioDeviceView {
            id: id.to_string(),
            name: name.to_string(),
            channels: 2,
            default_sample_rate: 48_000,
        }
    }

    #[test]
    fn the_default_audio_selection_is_the_system_monitor() {
        let selection = AudioSelectionView::default();

        assert!(selection.is_pipewire());
        assert!(!selection.is_default_input());
        assert_eq!(selection.describe(), "System audio (PipeWire monitor)");
        assert!(!selection.matches_device(&device("alsa:usb", "USB Microphone")));
    }

    #[test]
    fn input_selection_matches_by_id_or_name_and_marks_the_default() {
        let usb = device("alsa:usb", "USB Microphone");
        let mut selection = AudioSelectionView {
            source: "input".to_string(),
            device: Some("ALSA:USB".to_string()),
            error: None,
        };

        assert!(
            selection.matches_device(&usb),
            "ids match case-insensitively"
        );
        assert!(!selection.is_default_input());

        selection.device = Some("usb microphone".to_string());
        assert!(selection.matches_device(&usb), "names match too");

        selection.device = None;
        assert!(selection.is_default_input());
        assert_eq!(selection.describe(), "Default input (system default)");
        assert!(!selection.matches_device(&usb));

        selection.source = "pipewire".to_string();
        selection.device = Some(usb.id.clone());
        assert!(
            !selection.matches_device(&usb),
            "a device is only meaningful for the input source"
        );
    }

    #[test]
    fn the_active_banner_names_the_system_sources_without_exposing_ids() {
        let devices = [device("alsa:usb", "USB Microphone")];
        let pipewire = AudioSelectionView::default();

        assert_eq!(pipewire.active_label(&devices), "System audio");
        assert_eq!(
            pipewire.active_subtitle(&devices),
            "Captures what your speakers play"
        );
        assert_eq!(
            pipewire.active_detail(&devices),
            "PipeWire monitor · pipewire"
        );
        assert!(pipewire.active_device(&devices).is_none());

        let default_input = AudioSelectionView {
            source: "input".to_string(),
            device: None,
            error: None,
        };
        assert_eq!(default_input.active_label(&devices), "System default input");
        assert_eq!(
            default_input.active_subtitle(&devices),
            "Follows the system's default device"
        );
        assert_eq!(
            default_input.active_detail(&devices),
            "System default · input"
        );
    }

    #[test]
    fn the_active_banner_resolves_the_friendly_device_name_and_details() {
        let usb = device("alsa:usb", "USB Microphone");
        let devices = [usb.clone()];
        let selection = AudioSelectionView {
            source: "input".to_string(),
            device: Some("ALSA:USB".to_string()),
            error: None,
        };

        assert_eq!(selection.active_device(&devices), Some(&usb));
        assert_eq!(selection.active_label(&devices), "USB Microphone");
        assert_eq!(
            selection.active_subtitle(&devices),
            "Microphone or audio interface"
        );
        assert_eq!(
            selection.active_detail(&devices),
            "2 ch · 48 kHz · alsa:usb"
        );

        let mut fractional = usb.clone();
        fractional.default_sample_rate = 44_100;
        assert_eq!(
            AudioSelectionView {
                source: "input".to_string(),
                device: Some(fractional.id.clone()),
                error: None,
            }
            .active_detail(&[fractional]),
            "2 ch · 44.1 kHz · alsa:usb"
        );
    }

    #[test]
    fn an_unplugged_device_still_gets_an_honest_banner() {
        let selection = AudioSelectionView {
            source: "input".to_string(),
            device: Some("alsa:gone".to_string()),
            error: None,
        };

        assert_eq!(selection.active_label(&[]), "Input device");
        assert_eq!(
            selection.active_subtitle(&[]),
            "The selected device is not currently connected"
        );
        assert_eq!(selection.active_detail(&[]), "input · alsa:gone");
    }

    #[test]
    fn expression_and_tween_lookups_use_the_input_id() {
        let snapshot = EngineSnapshot {
            expressions: vec![ExpressionView {
                id: "expr-1".to_string(),
                input_id: "input-alpha".to_string(),
                expression: "value * 2".to_string(),
                evaluate_every_frame: true,
            }],
            tweens: HashMap::from([(
                "input-tween".to_string(),
                TweenView {
                    id: "input-tween".to_string(),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        assert_eq!(
            snapshot
                .expression_for_input("input-alpha")
                .map(|assignment| assignment.expression.as_str()),
            Some("value * 2")
        );
        assert!(snapshot.expression_for_input("input-missing").is_none());
        assert!(snapshot.tween_for_input("input-tween").is_some());
        assert!(snapshot.tween_for_input("input-alpha").is_none());
    }
}
