//! UI-local state and the action/command queues.
//!
//! Immediate mode keeps the UI stateless per frame except for durable
//! interaction state (selection, focus, drag sources) and the command queues
//! the app drains after drawing.

use std::collections::VecDeque;
use std::path::PathBuf;

use serde_json::Value;

use crate::file_dialog::{FileDialogAction, FileDialogState};
use crate::library_drag;
use crate::model::{
    EngineSnapshot, LibraryKind, MediaAction, MediaAssetView, ModuleMetaView, PreviewRequest,
    PropView, join_library_folder, normalise_library_folder, parent_library_folder,
};
use crate::panels::scene_grid::SceneGridState;
use crate::widgets::{DrawCurve, RangeControl, TweenCurve};

/// One engine action emitted by the UI.
#[derive(Clone, Debug, PartialEq)]
pub struct UiAction {
    pub action: String,
    pub payload: Value,
}

impl UiAction {
    pub fn new(action: impl Into<String>, payload: Value) -> Self {
        Self {
            action: action.into(),
            payload,
        }
    }

    /// A composite convenience handled by the native shell: create the module
    /// and append it to `group_id` in one UI gesture.
    pub fn add_module_to_group(module_name: &str, group_id: &str) -> Self {
        Self::new(
            "ui/addModuleToGroup",
            serde_json::json!({ "moduleName": module_name, "groupId": group_id }),
        )
    }

    /// Move an active Module to `to_index` in another Group (or within its
    /// own). `to_index` is the final index after the Module leaves its source
    /// Group; the engine clamps it to the destination length.
    pub fn move_module(
        module_id: &str,
        from_group_id: &str,
        to_group_id: &str,
        to_index: usize,
    ) -> Self {
        Self::new(
            "groups/MOVE_MODULE",
            serde_json::json!({
                "moduleId": module_id,
                "fromGroupId": from_group_id,
                "toGroupId": to_group_id,
                "toIndex": to_index,
            }),
        )
    }
}

/// The `modules/updateProp` action that writes a texture value.
///
/// Both assignment paths (a Library click and a Library drag/drop) go through
/// this builder, so the payload the engine receives is identical to the one
/// the manual path editor writes.
pub fn texture_prop_action(module_id: &str, prop: &str, value: Value) -> UiAction {
    UiAction::new(
        "modules/updateProp",
        serde_json::json!({
            "moduleId": module_id,
            "prop": prop,
            "data": { "value": value },
        }),
    )
}

/// A texture prop the Library tab is currently picking for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryAssignmentTarget {
    pub module_id: String,
    pub module_name: String,
    pub prop_name: String,
    pub kind: LibraryKind,
}

impl LibraryAssignmentTarget {
    /// The banner line the Library panel shows while armed.
    pub fn describe(&self) -> String {
        format!(
            "Pick {} for {} / {}",
            self.kind.title().to_lowercase(),
            self.module_name,
            self.prop_name
        )
    }
}

/// An application command that is not an engine action.
#[derive(Clone, Debug, PartialEq)]
pub enum UiCommand {
    Quit,
    OpenOutputWindow,
    CloseOutputWindow,
    ToggleOutputFullscreen,
    SaveSnapshot,
    SavePreset,
    FlushLogs,
    /// Open the in-app picker for this action. The native app resolves the
    /// starting folder from the active media profile.
    ShowFileDialog(FileDialogAction),
    /// The picker (or a command-line open) produced a validated path.
    FileChosen {
        action: FileDialogAction,
        path: PathBuf,
    },
    /// Create `<userData>/media/<name>` with the Electron profile folders.
    CreateProfile(String),
    /// Switch the active media profile and rescan the user ISF folders.
    SwitchProfile(String),
    /// Re-scan the Library tab's image/video/palette folders.
    RescanLibrary,
    /// Open the Library's current folder (`<profile>/<kind>/<relative>`) in
    /// the platform file manager.
    OpenLibraryFolder {
        folder: String,
        relative: String,
    },
    /// Open `<userData>/media` in the platform file manager.
    OpenMediaFolder,
    /// Open `<userData>/logs` in the platform file manager.
    OpenLogsFolder,
    /// Switch native audio capture live. `source` is `pipewire` or `input`;
    /// an `input` with no `device` selects the host default input.
    SetAudioSource {
        source: String,
        device: Option<String>,
    },
    /// Re-enumerate cpal input devices (hot-plug).
    RefreshAudioDevices,
    /// Turn the opt-in JavaScript plugin host on/off. Enabling without recorded
    /// consent is rejected by the runtime; the panel asks first.
    SetPluginsEnabled(bool),
    /// Record or withdraw the plugin sandbox consent. Withdrawing consent also
    /// disables the host (the runtime enforces that).
    SetPluginConsent(bool),
    /// Run one discovered plugin or stop it, by absolute script path.
    SetPluginEnabled {
        path: String,
        enabled: bool,
    },
    /// Re-read the plugin folders (new/removed/edited scripts).
    RescanPlugins,
    /// Force every discovered plugin through load + `init` again.
    ReloadPlugins,
    /// Write one plugin prop through `PluginHost::set_prop`. The runtime
    /// coerces the value against the descriptor schema before persisting it.
    SetPluginProp {
        path: String,
        prop: String,
        value: Value,
    },
    /// Drop the runtime's retained `plugin: …` panel lines.
    ClearPluginLogs,
}

/// P0 framework-spike evidence and the go/no-go state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct P0State {
    pub range_control_ok: bool,
    pub scene_cell_ok: bool,
    pub shared_texture_ok: bool,
    pub output_window_ok: bool,
    pub lavapipe_fps: Option<f32>,
    pub reference_fps: Option<f32>,
}

impl P0State {
    /// `GO` once the shared-texture path and the output window are live and
    /// the measured frame rate is at least `min_fps`.
    pub fn verdict(&self, fps: f32, min_fps: f32) -> bool {
        self.shared_texture_ok && self.output_window_ok && fps >= min_fps
    }

    pub fn checklist(&self, fps: f32) -> Vec<(&'static str, bool, String)> {
        vec![
            (
                "egui + engine share one wgpu device",
                self.shared_texture_ok,
                "register_native_texture from the compositor main texture".to_string(),
            ),
            (
                "RangeControl widget",
                self.range_control_ok,
                "slider + drag value + typed value + E/T badges".to_string(),
            ),
            (
                "Scenes-grid cell widget",
                self.scene_cell_ok,
                "hover preview, timing badge, Ctrl+drag copy, context menu".to_string(),
            ),
            (
                "Output window presented by the compositor",
                self.output_window_ok,
                "second winit surface on the same device".to_string(),
            ),
            (
                "Main-loop frame rate",
                fps >= 55.0,
                format!("{fps:.1} fps (target >= 55)"),
            ),
        ]
    }
}

/// The durable native UI state.
pub struct UiState {
    pub selected_group: Option<String>,
    pub selected_module: Option<String>,
    pub selected_scene: Option<String>,
    pub selected_gallery_module: Option<String>,
    /// Whether the startup Group focus has run. It runs once, so clearing the
    /// selection later (e.g. after removing the focused Group) stays cleared.
    initial_focus_done: bool,
    /// Inline Group-name draft and its owner.
    pub group_name_draft: String,
    pub group_name_draft_id: Option<String>,
    pub gallery_category: GalleryCategory,
    pub search: String,
    pub scene_grid: SceneGridState,
    pub actions: Vec<UiAction>,
    pub commands: Vec<UiCommand>,
    pub preview_texture: Option<egui::TextureId>,
    pub preview_size: [usize; 2],
    /// Hover-preview requests drained by the shell after drawing.
    pub preview_requests: Vec<PreviewRequest>,
    /// The last Gallery tile we requested a preview for, so a hover does not
    /// enqueue one request per frame.
    pub last_gallery_request: Option<std::time::Instant>,
    pub gallery_hover: Option<String>,
    /// The Gallery tile whose name is marqueeing and the `egui` clock time its
    /// hover started (seconds). Kept apart from `gallery_hover`, which drives
    /// preview requests, and cleared when the pointer leaves the tile so the
    /// next hover restarts the name at its left edge.
    pub gallery_name_hover: Option<(String, f64)>,
    pub output_open: bool,
    pub show_p0: bool,
    pub show_about: bool,
    pub show_shortcuts: bool,
    pub search_focus: bool,
    /// One-shot focus request for the Ctrl+K palette (separate from the
    /// Gallery search field).
    pub palette_focus: bool,
    pub link_editor: Option<LinkEditor>,
    /// The tween curve window currently open, if any.
    pub tween_editor: Option<TweenCurveEditor>,
    /// The drawn-curve window currently open, if any.
    pub draw_editor: Option<DrawCurveEditor>,
    pub show_bpm: bool,
    pub show_search: bool,
    pub command_query: String,
    pub command_selection: usize,
    pub bpm_source: String,
    pub tap_times: Vec<std::time::Instant>,
    pub side_panel_tab: SidePanelTab,
    /// The tab an in-flight right-panel drag is carrying, refreshed at the
    /// start of every frame from egui's drag-and-drop payload.
    pub tab_drag: Option<crate::tab_drag::TabDrag>,
    /// A one-frame request to place a floated tab's window at the position it
    /// was dropped, before the user moves or resizes it.
    pub tab_float_target: Option<(SidePanelTab, crate::layout::WindowBounds)>,
    /// Panel sizes, floating-window bounds and right-panel tab placements.
    pub layout: crate::layout::LayoutState,
    /// MediaHub operations drained by the shell after drawing.
    pub media_actions: Vec<MediaAction>,
    /// The Library tab's selected media folder.
    pub library_kind: LibraryKind,
    /// The current subfolder scope inside the selected media folder. Empty is
    /// the kind's root and lists every nested asset.
    pub library_folder: String,
    /// The asset selected by a click (media-root-relative path).
    pub selected_library_asset: Option<String>,
    /// A texture prop waiting for a library pick.
    pub library_target: Option<LibraryAssignmentTarget>,
    pub p0: P0State,
    pub inspector_probe: RangeControl,
    pub inspector_probe_value: f64,
    /// The most recent engine error rendered in the status bar/toast list.
    pub last_error: Option<String>,
    pub status: StatusState,
    /// Rolling frame-rate history for the status sparkline.
    pub fps_history: VecDeque<f32>,
    /// The NDI output-name draft. `None` seeds from the snapshot; typing goes
    /// into the draft so a once-per-second NDI refresh cannot fight the field.
    pub ndi_name_draft: Option<String>,
    /// The OSC add-listener form.
    pub osc_host_draft: String,
    pub osc_port_draft: String,
    pub osc_form_error: Option<String>,
    /// The open in-app file picker, if any.
    pub file_dialog: Option<FileDialogState>,
    /// The "New project profile" modal.
    pub show_profile_dialog: bool,
    pub profile_name: String,
    pub profile_error: Option<String>,
    /// The JavaScript-plugin consent modal, shown by the first enable attempt.
    pub show_plugin_consent: bool,
}

/// Which panel the right side shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidePanelTab {
    #[default]
    Inspector,
    Media,
    Library,
    Audio,
    Info,
    Plugins,
    Midi,
    Ndi,
    Osc,
}

impl SidePanelTab {
    /// Every right-panel tab, in strip order.
    pub const ALL: [Self; 9] = [
        Self::Inspector,
        Self::Media,
        Self::Library,
        Self::Audio,
        Self::Info,
        Self::Plugins,
        Self::Midi,
        Self::Ndi,
        Self::Osc,
    ];
    /// The number of right-panel tabs.
    pub const COUNT: usize = Self::ALL.len();

    /// The tab's index in [`SidePanelTab::ALL`].
    pub fn index(self) -> usize {
        match self {
            Self::Inspector => 0,
            Self::Media => 1,
            Self::Library => 2,
            Self::Audio => 3,
            Self::Info => 4,
            Self::Plugins => 5,
            Self::Midi => 6,
            Self::Ndi => 7,
            Self::Osc => 8,
        }
    }

    /// The stable id persisted in `native-layout.json`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Inspector => "inspector",
            Self::Media => "media",
            Self::Library => "library",
            Self::Audio => "audio",
            Self::Info => "info",
            Self::Plugins => "plugins",
            Self::Midi => "midi",
            Self::Ndi => "ndi",
            Self::Osc => "osc",
        }
    }

    /// The panel title used by the strip, dock headers and windows.
    pub fn title(self) -> &'static str {
        match self {
            Self::Inspector => "Inspector",
            Self::Media => "Media",
            Self::Library => "Library",
            Self::Audio => "Audio",
            Self::Info => "Info",
            Self::Plugins => "Plugins",
            Self::Midi => "MIDI",
            Self::Ndi => "NDI",
            Self::Osc => "OSC",
        }
    }

    /// Parse a persisted key; unknown ids are ignored.
    pub fn from_key(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.key() == value)
    }
}

/// The default OSC bind host the native app listens on at startup
/// (`application/index.js` creates the same listener).
pub const DEFAULT_OSC_HOST: &str = "0.0.0.0";

/// The default OSC port (`application/index.js`).
pub const DEFAULT_OSC_PORT: u16 = 3333;

/// The audio features `AudioFeatures.vue` offers, in menu order.
///
/// `none` removes the link; `bpm` is a state link on `beats.bpm`; `kick` is a
/// mutation link on `beats/SET_KICK`; every other entry is a
/// `meyda/getFeature` getter.
pub const AUDIO_FEATURES: &[&str] = &[
    "none",
    "bpm",
    "kick",
    "bass",
    "mid",
    "treble",
    "rms",
    "zcr",
    "energy",
    "spectralCentroid",
    "spectralFlatness",
    "spectralSlope",
    "spectralRolloff",
    "spectralSpread",
    "spectralSkewness",
    "spectralKurtosis",
    "perceptualSpread",
    "perceptualSharpness",
];

/// `MAX_SMOOTHING` from `application/worker/audio-features.js`.
pub const MAX_SMOOTHING: f64 = 1.0;

/// `SMOOTHING_STEP` from `application/worker/audio-features.js`.
pub const SMOOTHING_STEP: f64 = 0.001;

/// The one-shot MIDI Learn arm a [`LinkEditor`] is waiting on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MidiLearnDraft {
    /// The `last_learn.id` observed when Learn was clicked. Only a newer
    /// result can answer this arm, so a result left in the snapshot can never
    /// create a second link.
    pub floor: Option<u64>,
    /// The engine arm id, once the snapshot reported it. It lets the editor
    /// notice that another input replaced its arm.
    pub arm: Option<u64>,
}

/// The focused-input editor's title, `<owner>: <control>`.
///
/// Legacy `ActiveModule.vue`/`Group.vue` dispatch
/// `inputs/setFocusedInput` with `` `${name}: ${title}` ``, so the Input
/// Config panel reads `Ball: Amount`. The native link rows only carry the
/// control label and the input id; the owner name is resolved from the
/// snapshot so every module prop, component sub-input and Group control keeps
/// the legacy title shape.
pub fn focused_input_title(input_id: &str, control: &str, snapshot: &EngineSnapshot) -> String {
    let module = snapshot.modules.iter().find(|module| {
        module.alpha_input_id.as_deref() == Some(input_id)
            || module.composite_operation_input_id.as_deref() == Some(input_id)
            || module.props.iter().any(|prop| {
                prop.input_id.as_deref() == Some(input_id)
                    || prop
                        .components
                        .iter()
                        .any(|component| component.input_id == input_id)
            })
    });
    if let Some(module) = module {
        return format!("{}: {control}", module.module_name);
    }

    let group = snapshot.groups.iter().find(|group| {
        group.alpha_input_id.as_deref() == Some(input_id)
            || group.composite_operation_input_id.as_deref() == Some(input_id)
    });
    if let Some(group) = group {
        // The shell labels Group link rows `Group alpha`/`Group blend`; the
        // legacy title is the Group name plus the capitalised control
        // (`Background: Alpha`).
        let control = control.strip_prefix("Group ").unwrap_or(control);
        let mut chars = control.chars();
        let control = match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        };
        return format!("{}: {control}", group.name);
    }

    control.to_string()
}

/// The focused-input link editor's draft values.
///
/// Legacy `InputConfig.vue` is a focused-input panel with Min/Max at the top
/// and Audio/MIDI/Tween/OSC/Expression sections. The native editor keeps the
/// same draft semantics; each section's pure payload builder is unit-tested.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkEditor {
    pub input_id: String,
    pub title: String,
    pub kind: String,
    pub source: String,
    pub location: String,
    /// JSON array text for getter links.
    pub args: String,
    pub min: f64,
    pub max: f64,
    /// Mutation matcher (`beats/SET_KICK` for the kick feature).
    pub matcher: Option<Value>,
    /// The audio feature selected in the Audio section.
    pub feature: String,
    /// Smoothing id carried in `args[1]` (minted client-side).
    pub smoothing_id: Option<String>,
    /// Smoothing amount carried in `args[2]` (`0` means no smoothing).
    pub smoothing_value: f64,
    /// Expression text (`"value"` is the identity/no-op).
    pub expression: String,
    /// The assignment id, when the input already has an expression.
    pub expression_id: Option<String>,
    pub evaluate_every_frame: bool,
    /// Set after Clear so the snapshot's still-live assignment is not
    /// re-adopted before the engine processes `expressions/remove`.
    pub expression_cleared: bool,
    /// The tween descriptor for this input, when one exists.
    pub tween: Option<crate::TweenView>,
    /// Set after Remove so a one-frame-old snapshot does not re-adopt the
    /// tween descriptor the engine is about to drop.
    pub tween_cleared: bool,
    /// The pending one-shot MIDI Learn arm, while “Waiting for MIDI…” shows.
    pub midi_learn: Option<MidiLearnDraft>,
    pub exists: bool,
    pub error: Option<String>,
}

impl LinkEditor {
    pub fn audio(input_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            input_id: input_id.into(),
            title: title.into(),
            kind: "getter".to_string(),
            source: "meyda".to_string(),
            location: "meyda/getFeature".to_string(),
            args: "[\"bass\"]".to_string(),
            min: 0.0,
            max: 1.0,
            matcher: None,
            feature: "bass".to_string(),
            smoothing_id: None,
            smoothing_value: 0.0,
            expression: "value".to_string(),
            expression_id: None,
            evaluate_every_frame: false,
            expression_cleared: false,
            tween: None,
            tween_cleared: false,
            midi_learn: None,
            exists: false,
            error: None,
        }
    }

    pub fn from_link(
        input_id: impl Into<String>,
        title: impl Into<String>,
        link: &crate::InputLinkView,
    ) -> Self {
        let source = link.source.clone().unwrap_or_else(|| "state".to_string());
        let feature = if source == "meyda" {
            link.args
                .as_ref()
                .and_then(|args| args.as_array())
                .and_then(|args| args.first())
                .and_then(|value| value.as_str())
                .unwrap_or("none")
                .to_string()
        } else if source == "bpm" {
            "bpm".to_string()
        } else {
            "none".to_string()
        };
        let (smoothing_id, smoothing_value) = link
            .args
            .as_ref()
            .and_then(|args| args.as_array())
            .filter(|args| args.len() > 2)
            .map(|args| {
                (
                    args.get(1).and_then(|value| {
                        value
                            .as_str()
                            .map(str::to_string)
                            .or_else(|| value.as_u64().map(|number| number.to_string()))
                    }),
                    args.get(2).and_then(Value::as_f64).unwrap_or(0.0),
                )
            })
            .unwrap_or((None, 0.0));
        // A BPM link passes the tempo through unchanged. Force the identity
        // bounds so a link saved with a ranged meyda link's min/max cannot
        // scale 174 BPM into the thousands.
        let (min, max) = if source == "bpm" {
            (0.0, 1.0)
        } else {
            (link.min.unwrap_or(0.0), link.max.unwrap_or(1.0))
        };
        Self {
            input_id: input_id.into(),
            title: title.into(),
            kind: link.kind.clone(),
            source,
            location: link.location.clone().unwrap_or_default(),
            args: link
                .args
                .as_ref()
                .map(|args| args.to_string())
                .unwrap_or_else(|| "[]".to_string()),
            min,
            max,
            matcher: link.matcher.clone(),
            feature,
            smoothing_id,
            smoothing_value,
            expression: "value".to_string(),
            expression_id: None,
            evaluate_every_frame: false,
            expression_cleared: false,
            tween: None,
            tween_cleared: false,
            midi_learn: None,
            exists: true,
            error: None,
        }
    }

    /// A complete editor draft for `input_id`, preloading its link, tween and
    /// expression assignment from the snapshot.
    pub fn for_input(input_id: &str, title: impl Into<String>, snapshot: &EngineSnapshot) -> Self {
        let mut editor = match snapshot.input_links.get(input_id) {
            Some(link) => Self::from_link(input_id, title, link),
            None => Self::audio(input_id, title),
        };
        editor.tween = snapshot.tween_for_input(input_id).cloned();
        if let Some(assignment) = snapshot.expression_for_input(input_id) {
            editor.expression = assignment.expression.clone();
            editor.expression_id = Some(assignment.id.clone());
            editor.evaluate_every_frame = assignment.evaluate_every_frame;
        }
        editor
    }

    /// The audio feature named by the getter args, if any.
    pub fn audio_feature(&self) -> String {
        self.feature.clone()
    }

    /// Select an audio feature, updating the kind/source/location/matcher the
    /// JS `makeLink` branches on.
    pub fn set_audio_feature(&mut self, feature: &str) {
        self.feature = feature.to_string();
        match feature {
            "none" => {
                self.source = "meyda".to_string();
                self.kind = "getter".to_string();
                self.location = "meyda/getFeature".to_string();
                self.matcher = None;
                self.args = "[]".to_string();
            }
            "kick" => {
                self.kind = "mutation".to_string();
                self.source = "meyda".to_string();
                self.location = "beats.kick".to_string();
                self.matcher = Some(serde_json::json!({ "type": "beats/SET_KICK" }));
                self.args = serde_json::json!(["kick"]).to_string();
            }
            "bpm" => {
                self.kind = "state".to_string();
                self.source = "bpm".to_string();
                self.location = "beats.bpm".to_string();
                self.matcher = None;
                self.args = "[]".to_string();
                // Bounds map 0..1 onto themselves; reset a ranged meyda
                // link's min/max so the tempo is not scaled.
                self.min = 0.0;
                self.max = 1.0;
            }
            _ => {
                self.kind = "getter".to_string();
                self.source = "meyda".to_string();
                self.location = "meyda/getFeature".to_string();
                self.matcher = None;
                self.args = self.audio_args().to_string();
            }
        }
    }

    /// Use a state path (`Kind` is `state`; args are unused).
    pub fn use_state(&mut self, location: impl Into<String>) {
        self.kind = "state".to_string();
        self.source = "state".to_string();
        self.location = location.into();
        self.matcher = None;
        self.args = "[]".to_string();
        self.feature = "none".to_string();
    }

    /// Use a MIDI state path (`midi.data.<device>.<channel>`).
    pub fn use_midi(&mut self, location: impl Into<String>) {
        self.use_state(location);
        self.source = "midi".to_string();
    }

    /// Use an OSC receive path (`osc.data.<address>`).
    pub fn use_osc(&mut self, location: impl Into<String>) {
        self.use_state(location);
        self.source = "osc".to_string();
    }

    /// Arm a one-shot MIDI Learn for this input from the current snapshot.
    ///
    /// `floor` records the newest result the editor has already seen, so a
    /// stale result that is still in the snapshot cannot create a link for a
    /// later arm. The caller queues `midi/learn`; this only changes the draft.
    pub fn start_midi_learn(&mut self, snapshot: &EngineSnapshot) {
        self.midi_learn = Some(MidiLearnDraft {
            floor: snapshot.midi.last_learn.as_ref().map(|result| result.id),
            arm: None,
        });
        self.error = None;
    }

    /// Stop showing “Waiting for MIDI…” (Cancel, Forget or the editor closing).
    pub fn cancel_midi_learn(&mut self) {
        self.midi_learn = None;
    }

    pub fn is_waiting_for_midi(&self) -> bool {
        self.midi_learn.is_some()
    }

    /// Adopt a resolved Learn, returning the actions to queue.
    ///
    /// Electron's `MIDI.vue` dispatches `inputs/createInputLink` with kind
    /// `mutation`, source `midi`, the quoted `channelData` location and the
    /// `midi/WRITE_DATA` matcher. The native editor additionally releases a
    /// stale smoothing id (and tween descriptor) from the link it replaces,
    /// which the JS path leaks.
    pub fn sync_midi_learn(&mut self, snapshot: &EngineSnapshot) -> Vec<UiAction> {
        let Some(mut draft) = self.midi_learn else {
            return Vec::new();
        };

        if let Some(result) = snapshot.midi.last_learn.as_ref()
            && result.input_id == self.input_id
            && Some(result.id) > draft.floor
        {
            self.midi_learn = None;
            return self.apply_midi_result(result, snapshot);
        }

        if let Some(arm) = snapshot.midi.learning.as_ref() {
            if arm.input_id == self.input_id {
                draft.arm = Some(arm.id);
                self.midi_learn = Some(draft);
            } else {
                // One arm at a time: a Learn for another input replaced ours.
                self.midi_learn = None;
                self.error = Some(format!(
                    "MIDI Learn moved to input {}; click Learn again",
                    arm.input_id
                ));
            }
            return Vec::new();
        }

        // The engine arm we saw is gone without a result (cancelled from the
        // MIDI panel or the editor that owned it closed).
        if draft.arm.is_some() {
            self.midi_learn = None;
        }
        Vec::new()
    }

    fn apply_midi_result(
        &mut self,
        result: &crate::MidiLearnResultView,
        snapshot: &EngineSnapshot,
    ) -> Vec<UiAction> {
        let mut actions = Vec::new();
        let existing = snapshot.input_links.get(&self.input_id);
        let smoothing_id = self
            .smoothing_id
            .take()
            .or_else(|| existing.and_then(crate::InputLinkView::smoothing_id));
        if let Some(id) = smoothing_id {
            actions.push(UiAction::new("meyda/removeSmoothingId", Value::String(id)));
        }
        if matches!(
            existing.and_then(|link| link.source.as_deref()),
            Some("tween" | "draw")
        ) {
            // `createInputLink` replaces the record; drop the descriptor the
            // replaced tween/draw link owned instead of leaving an orphan.
            actions.push(UiAction::new(
                "tweens/REMOVE_TWEEN",
                serde_json::json!({ "id": self.input_id }),
            ));
            self.tween = None;
            self.tween_cleared = true;
        }

        self.kind = "mutation".to_string();
        self.source = "midi".to_string();
        self.location = midi_link_location(&result.device, result.channel, result.type_);
        self.matcher = Some(midi_link_matcher(
            &result.device,
            result.channel,
            result.type_,
        ));
        self.args = "[]".to_string();
        self.feature = "none".to_string();
        self.smoothing_value = 0.0;
        self.min = 0.0;
        self.max = 1.0;
        self.exists = true;
        self.error = None;
        actions.push(UiAction::new(
            "inputs/createInputLink",
            self.link_payload()
                .expect("a learned MIDI link has a valid payload"),
        ));
        actions
    }

    /// The JS smoothing slider is inverted: `MAX_SMOOTHING - smoothingValue`.
    pub fn smoothing_slider_value(&self) -> f64 {
        MAX_SMOOTHING - self.smoothing_value
    }

    pub fn set_smoothing_from_slider(&mut self, slider: f64) {
        self.smoothing_value = MAX_SMOOTHING - slider;
    }

    /// The `args` array for the selected audio feature.
    pub fn audio_args(&self) -> Value {
        match self.feature.as_str() {
            "kick" => serde_json::json!(["kick"]),
            "bpm" | "none" => serde_json::json!([]),
            feature => {
                if self.smoothing_value > 0.0
                    && let Some(id) = &self.smoothing_id
                {
                    return serde_json::json!([feature, id, self.smoothing_value]);
                }
                serde_json::json!([feature])
            }
        }
    }

    /// The complete `inputs/createInputLink` payload for the current draft.
    ///
    /// Audio links are rebuilt from the feature/smoothing state (exactly like
    /// `AudioFeatures.vue`'s `makeLink`), so a stale kick matcher can never
    /// survive a switch back to a getter.
    pub fn link_payload(&mut self) -> Result<Value, String> {
        let args = if self.source == "meyda" {
            if self.feature == "kick" {
                serde_json::json!(["kick"])
            } else {
                if self.smoothing_value > 0.0 && self.smoothing_id.is_none() {
                    self.smoothing_id = Some(mint_smoothing_id());
                }
                self.audio_args()
            }
        } else {
            self.parsed_args()?
        };
        let mut payload = serde_json::json!({
            "inputId": self.input_id,
            "type": self.kind,
            "source": self.source,
            "min": self.min,
            "max": self.max,
        });
        let object = payload.as_object_mut().expect("payload is an object");
        if !self.location.is_empty() {
            object.insert("location".to_string(), serde_json::json!(self.location));
        }
        if !args.is_null() {
            object.insert("args".to_string(), args);
        }
        if self.source == "meyda" && self.feature == "kick" {
            object.insert(
                "match".to_string(),
                serde_json::json!({ "type": "beats/SET_KICK" }),
            );
        } else if let Some(matcher) = &self.matcher {
            object.insert("match".to_string(), matcher.clone());
        }
        Ok(payload)
    }

    /// Actions for the editor's Save button.
    pub fn save_actions(&mut self) -> Result<Vec<UiAction>, String> {
        if self.source == "meyda" && self.feature == "none" {
            return Ok(self.remove_actions());
        }
        let mut actions = Vec::new();
        // Switching to BPM replaces a meyda link, so release any smoothing id
        // the previous audio link minted instead of leaking the entry.
        let release_smoothing = self.smoothing_id.is_some()
            && (self.source == "bpm"
                || (self.source == "meyda"
                    && (self.feature == "kick" || self.smoothing_value <= 0.0)));
        if release_smoothing && let Some(action) = self.release_smoothing_action() {
            actions.push(action);
        }
        actions.push(UiAction::new(
            "inputs/createInputLink",
            self.link_payload()?,
        ));
        Ok(actions)
    }

    /// Actions for Remove: drop the link and release its smoothing id.
    pub fn remove_actions(&mut self) -> Vec<UiAction> {
        let mut actions = Vec::new();
        if let Some(action) = self.release_smoothing_action() {
            actions.push(action);
        }
        actions.push(UiAction::new(
            "inputs/removeInputLink",
            serde_json::json!({ "inputId": self.input_id }),
        ));
        self.exists = false;
        actions
    }

    /// Zero the smoothing amount and release the minted id.
    pub fn clear_smoothing(&mut self) -> Option<UiAction> {
        self.smoothing_value = 0.0;
        if self.source == "meyda" {
            self.args = self.audio_args().to_string();
        }
        self.release_smoothing_action()
    }

    fn release_smoothing_action(&mut self) -> Option<UiAction> {
        self.smoothing_id.take().map(|id| {
            // `meyda/removeSmoothingId` takes the id itself, not an object.
            UiAction::new("meyda/removeSmoothingId", Value::String(id))
        })
    }

    /// Actions for the Expression section's Save button.
    ///
    /// `"value"` is the identity the engine removes on (`expressions.js`);
    /// an empty source is treated the same way.
    pub fn expression_actions(&self) -> Vec<UiAction> {
        let expression = self.expression.trim();
        if expression.is_empty() || expression == "value" {
            return self.clear_expression_actions();
        }
        match &self.expression_id {
            Some(id) => vec![UiAction::new(
                "expressions/update",
                serde_json::json!({ "id": id, "expression": self.expression }),
            )],
            None => vec![UiAction::new(
                "expressions/create",
                serde_json::json!({
                    "expression": self.expression,
                    "inputId": self.input_id,
                    "evaluateEveryFrame": self.evaluate_every_frame,
                }),
            )],
        }
    }

    /// Actions for the Expression section's Clear button.
    pub fn clear_expression_actions(&self) -> Vec<UiAction> {
        self.expression_id
            .as_ref()
            .map(|id| {
                vec![UiAction::new(
                    "expressions/remove",
                    serde_json::json!({ "id": id }),
                )]
            })
            .unwrap_or_default()
    }

    /// `expressions/updateEvaluation` when the checkbox is toggled.
    pub fn evaluation_action(&self) -> Option<UiAction> {
        self.expression_id.as_ref().map(|id| {
            UiAction::new(
                "expressions/updateEvaluation",
                serde_json::json!({
                    "id": id,
                    "evaluateEveryFrame": self.evaluate_every_frame,
                }),
            )
        })
    }

    /// Adopt the authoritative expression assignment for this input.
    ///
    /// A Clear latches `expression_cleared` until the engine drops the
    /// assignment, so the one-frame-old snapshot cannot resurrect the
    /// expression the user just removed.
    pub fn sync_expression(&mut self, assignment: Option<&crate::ExpressionView>) {
        match assignment {
            Some(assignment) if !self.expression_cleared => {
                if self.expression_id.is_none() {
                    self.expression_id = Some(assignment.id.clone());
                    if self.expression.trim() == "value" {
                        self.expression = assignment.expression.clone();
                    }
                }
                if self.expression_id.as_deref() == Some(assignment.id.as_str()) {
                    self.evaluate_every_frame = assignment.evaluate_every_frame;
                }
            }
            Some(_) => {}
            None => {
                self.expression_cleared = false;
                self.expression_id = None;
                self.evaluate_every_frame = false;
            }
        }
    }

    /// Clear the expression draft and wait for the engine to drop it.
    pub fn mark_expression_cleared(&mut self) {
        self.expression = "value".to_string();
        self.expression_id = None;
        self.evaluate_every_frame = false;
        self.expression_cleared = true;
    }

    pub fn parsed_args(&self) -> Result<Value, String> {
        if self.kind != "getter" {
            return Ok(Value::Null);
        }
        serde_json::from_str::<Value>(&self.args)
            .map_err(|error| format!("args must be a JSON array: {error}"))
    }
}

/// The keyframe handle currently being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TweenHandleDrag {
    /// Row index in the curve.
    pub index: usize,
    /// Channel index the vertical drag applies to.
    pub channel: usize,
}

/// The open tween curve editor (one window at a time).
///
/// `TweenControl` itself is rebuilt from the engine view every frame, so the
/// draft curve, drag and JSON text live here for the duration of the window.
#[derive(Clone, Debug, PartialEq)]
pub struct TweenCurveEditor {
    /// The Input id (and tween id) the window edits.
    pub input_id: String,
    /// The property label shown in the window title.
    pub title: String,
    /// Whether the window is open; closing it clears [`UiState::tween_editor`].
    pub open: bool,
    /// The edited keyframes (always explicit times, normalised on open).
    pub curve: TweenCurve,
    /// The handle under the pointer, when a drag is in progress.
    pub dragging: Option<TweenHandleDrag>,
    /// Pointer position of a press that might turn into an add click.
    pub pending_press: Option<egui::Pos2>,
    /// The editable JSON text.
    pub json: String,
    /// Whether `json` was typed by the user (so it must not be rewritten from
    /// the curve every frame).
    pub json_dirty: bool,
    /// The inline parse error, when the JSON is invalid.
    pub error: Option<String>,
}

impl TweenCurveEditor {
    /// Open an editor for `curve`.
    pub fn new(input_id: impl Into<String>, title: impl Into<String>, curve: TweenCurve) -> Self {
        Self {
            input_id: input_id.into(),
            title: title.into(),
            open: true,
            json: curve.to_json(),
            curve,
            dragging: None,
            pending_press: None,
            json_dirty: false,
            error: None,
        }
    }
}

/// The open freehand draw-curve editor (one window at a time).
///
/// The painted curve lives here for the duration of the window; the descriptor
/// is only dispatched when a stroke or Reset completes.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawCurveEditor {
    /// The Input id (and descriptor id) the window edits.
    pub input_id: String,
    /// The property label shown in the window title.
    pub title: String,
    /// Whether the window is open; closing it clears [`UiState::draw_editor`].
    pub open: bool,
    /// The painted points.
    pub curve: DrawCurve,
    /// The previous pointer `(progress, value)` while a freehand stroke is in
    /// flight (Shift+drag).
    pub stroke: Option<(f64, f64)>,
    /// The point being dragged in click-to-edit mode.
    pub dragging: Option<usize>,
    /// Pointer position of a press that might turn into an add click.
    pub pending_press: Option<egui::Pos2>,
    /// Whether the current gesture changed the curve.
    pub changed: bool,
}

impl DrawCurveEditor {
    /// Open an editor for `curve`.
    pub fn new(input_id: impl Into<String>, title: impl Into<String>, curve: DrawCurve) -> Self {
        Self {
            input_id: input_id.into(),
            title: title.into(),
            open: true,
            curve,
            stroke: None,
            dragging: None,
            pending_press: None,
            changed: false,
        }
    }
}

/// Mint a smoothing id client-side.
/// The engine's `meyda/getSmoothingId` exists but the native UI cannot await
/// an action result; the evaluator seeds an unseen id at `0` on first use
/// (`input_links.rs`), so a locally unique id is equivalent.
pub fn mint_smoothing_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("native-smoothing-{nanos}-{counter}")
}

/// The `MIDI.vue` location for a learned message:
/// `midi.devices['<id>-<name>-<manufacturer>'].channelData['<channel>']['<type>']`.
///
/// The device key and channel indexes are quoted exactly as the JS template
/// writes them, even though the channel/type indexes are numeric.
pub fn midi_link_location(device: &str, channel: i64, type_: i64) -> String {
    format!("midi.devices['{device}'].channelData['{channel}']['{type_}']")
}

/// The `MIDI.vue` mutation matcher for a learned message.
pub fn midi_link_matcher(device: &str, channel: i64, type_: i64) -> Value {
    serde_json::json!({
        "type": "midi/WRITE_DATA",
        "payload": { "id": device, "channel": channel, "type": type_ },
    })
}

/// Status-bar values mirrored from the engine snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusState {
    pub fps: f32,
    pub frame_ms: f32,
    pub resolution: [u32; 2],
    pub bpm: f64,
    pub adapter: String,
    pub backend: String,
    pub frames_presented: u64,
    pub audio_active: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        let mut inspector_probe = RangeControl::new("gain", 0.5, 0.0, 2.0, 0.001)
            .with_unit("")
            .with_default(0.5);
        inspector_probe.expression = true;
        inspector_probe.tween = true;
        Self {
            selected_group: None,
            selected_module: None,
            selected_scene: None,
            selected_gallery_module: None,
            initial_focus_done: false,
            group_name_draft: String::new(),
            group_name_draft_id: None,
            gallery_category: GalleryCategory::All,
            search: String::new(),
            scene_grid: SceneGridState::default(),
            actions: Vec::new(),
            commands: Vec::new(),
            preview_texture: None,
            preview_size: [0, 0],
            preview_requests: Vec::new(),
            last_gallery_request: None,
            gallery_hover: None,
            gallery_name_hover: None,
            output_open: false,
            show_p0: true,
            show_about: false,
            show_shortcuts: false,
            search_focus: false,
            palette_focus: false,
            link_editor: None,
            tween_editor: None,
            draw_editor: None,
            show_bpm: false,
            show_search: false,
            command_query: String::new(),
            command_selection: 0,
            bpm_source: "beatdetektor".to_string(),
            tap_times: Vec::new(),
            side_panel_tab: SidePanelTab::Inspector,
            tab_drag: None,
            tab_float_target: None,
            layout: crate::layout::LayoutState::default(),
            media_actions: Vec::new(),
            library_kind: LibraryKind::Image,
            library_folder: String::new(),
            selected_library_asset: None,
            library_target: None,
            p0: P0State::default(),
            inspector_probe,
            inspector_probe_value: 0.5,
            last_error: None,
            status: StatusState::default(),
            fps_history: VecDeque::new(),
            ndi_name_draft: None,
            osc_host_draft: DEFAULT_OSC_HOST.to_string(),
            osc_port_draft: DEFAULT_OSC_PORT.to_string(),
            osc_form_error: None,
            file_dialog: None,
            show_profile_dialog: false,
            profile_name: String::new(),
            profile_error: None,
            show_plugin_consent: false,
        }
    }

    /// Open the in-app picker at `start_dir`.
    pub fn open_file_dialog(&mut self, action: FileDialogAction, start_dir: impl Into<PathBuf>) {
        self.file_dialog = Some(FileDialogState::open(action, start_dir));
    }

    /// Open the "New project profile" modal with an empty name.
    pub fn open_profile_dialog(&mut self) {
        self.show_profile_dialog = true;
        self.profile_name.clear();
        self.profile_error = None;
    }

    /// Show the JavaScript-plugin consent dialog (the first enable attempt).
    pub fn open_plugin_consent(&mut self) {
        self.show_plugin_consent = true;
    }

    /// Accept the consent notice **and** turn the host on.
    ///
    /// The two commands are queued in order (`SetPluginConsent(true)` first),
    /// so the runtime never sees an enable without recorded consent even if
    /// the shell applies them one at a time.
    pub fn confirm_plugin_consent(&mut self) {
        self.commands.push(UiCommand::SetPluginConsent(true));
        self.commands.push(UiCommand::SetPluginsEnabled(true));
        self.show_plugin_consent = false;
    }

    /// Dismiss the consent dialog without changing the recorded consent.
    pub fn cancel_plugin_consent(&mut self) {
        self.show_plugin_consent = false;
    }

    /// Queue the profile named by the modal, or show why the name is unusable.
    pub fn queue_create_profile(&mut self) {
        let name = self.profile_name.trim().to_string();
        if name.is_empty() {
            self.profile_error = Some("Enter a profile name".to_string());
            return;
        }
        self.commands.push(UiCommand::CreateProfile(name));
        self.show_profile_dialog = false;
        self.profile_error = None;
    }

    /// Whether `tab` is on screen this frame: the active strip tab, or a tab
    /// placed in the dock column / a floating window (those are always shown).
    ///
    /// The NDI and OSC panels use this to poll the engine at 1 Hz instead of
    /// rebuilding their view every frame while the tab is hidden.
    pub fn is_tab_visible(&self, tab: SidePanelTab) -> bool {
        match self.layout.tabs.placement(tab) {
            crate::layout::TabPlacement::Tabs => self.side_panel_tab == tab,
            crate::layout::TabPlacement::Docked | crate::layout::TabPlacement::Floating => true,
        }
    }

    /// Update the preview texture registration.
    pub fn set_preview(&mut self, texture: egui::TextureId, width: usize, height: usize) {
        self.preview_texture = Some(texture);
        self.preview_size = [width, height];
        self.p0.shared_texture_ok = width > 0 && height > 0;
    }

    /// The preview texture was released (e.g. the compositor reset).
    pub fn clear_preview(&mut self) {
        self.preview_texture = None;
        self.preview_size = [0, 0];
        self.p0.shared_texture_ok = false;
    }

    /// Push one engine action.
    pub fn push_action(&mut self, action: impl Into<String>, payload: Value) {
        self.actions.push(UiAction::new(action, payload));
    }

    /// Open the focused-input link editor.
    ///
    /// Opening another input's editor closes the current one, so a pending
    /// MIDI Learn owned by the replaced editor is cancelled; only the editor
    /// can consume the result and a stale arm must not fire later.
    pub fn open_link_editor(&mut self, editor: LinkEditor) {
        if self
            .link_editor
            .as_ref()
            .is_some_and(LinkEditor::is_waiting_for_midi)
        {
            self.push_action("midi/cancelLearn", Value::Null);
        }
        self.link_editor = Some(editor);
    }

    /// Switch the Library folder. Changing kind returns to that kind's root;
    /// the folder is otherwise kept while the panel is open.
    pub fn select_library_kind(&mut self, kind: LibraryKind) {
        if self.library_kind != kind {
            self.library_kind = kind;
            self.library_folder.clear();
        }
    }

    /// Open a subfolder (relative to the kind root) in the Library panel.
    pub fn enter_library_folder(&mut self, folder: impl AsRef<str>) {
        self.library_folder = normalise_library_folder(folder.as_ref());
    }

    /// Enter an immediate child of the current Library folder.
    pub fn open_library_subfolder(&mut self, child: &str) {
        self.library_folder = join_library_folder(&self.library_folder, child);
    }

    /// Go up one Library folder (a no-op at the kind root).
    pub fn library_up(&mut self) {
        self.library_folder = parent_library_folder(&self.library_folder);
    }

    /// Mark a texture prop as waiting for a Library pick.
    ///
    /// Arms the panel and brings it to the front with the matching folder
    /// selected, so the "Use from library…" button is one click away from a
    /// thumbnail.
    pub fn arm_library_target(&mut self, target: LibraryAssignmentTarget) {
        if self.library_kind != target.kind {
            self.library_folder.clear();
        }
        self.library_kind = target.kind;
        self.library_target = Some(target);
        self.side_panel_tab = SidePanelTab::Library;
    }

    /// Cancel a pending Library assignment (the prop keeps its current value).
    pub fn clear_library_target(&mut self) {
        self.library_target = None;
    }

    /// Queue the prop write for a clicked Library asset when a target is armed.
    ///
    /// Returns whether an action was queued: no target, a mismatched kind, or a
    /// palette asset does nothing.
    pub fn assign_library_asset(&mut self, asset: &MediaAssetView) -> bool {
        let Some(target) = self.library_target.clone() else {
            return false;
        };
        if target.kind != asset.kind {
            return false;
        }
        let Some(value) = library_drag::texture_value_for_asset(asset.kind, &asset.path) else {
            return false;
        };
        self.selected_library_asset = Some(asset.path.clone());
        self.actions.push(texture_prop_action(
            &target.module_id,
            &target.prop_name,
            value,
        ));
        self.library_target = None;
        true
    }

    /// Take the queued engine actions.
    pub fn take_actions(&mut self) -> Vec<UiAction> {
        std::mem::take(&mut self.actions)
    }

    /// Ask the shell for a Gallery preview at most every `interval`.
    pub fn request_gallery_preview(
        &mut self,
        module_name: &str,
        with_output: bool,
        interval: std::time::Duration,
    ) {
        let now = std::time::Instant::now();
        let due = self
            .last_gallery_request
            .map(|last| now.saturating_duration_since(last) >= interval)
            .unwrap_or(true);
        if self.gallery_hover.as_deref() != Some(module_name) || due {
            self.gallery_hover = Some(module_name.to_string());
            self.last_gallery_request = Some(now);
            self.preview_requests.push(PreviewRequest::Module {
                module_name: module_name.to_string(),
                with_output,
            });
        }
    }

    /// The shell drains at most one preview request per frame.
    pub fn take_preview_requests(&mut self) -> Vec<PreviewRequest> {
        std::mem::take(&mut self.preview_requests)
    }

    pub fn push_media_action(&mut self, action: MediaAction) {
        self.media_actions.push(action);
    }

    pub fn take_media_actions(&mut self) -> Vec<MediaAction> {
        std::mem::take(&mut self.media_actions)
    }

    /// Queue an application command.
    pub fn queue(&mut self, command: UiCommand) {
        self.commands.push(command);
    }

    /// Take the queued application commands.
    pub fn take_commands(&mut self) -> Vec<UiCommand> {
        std::mem::take(&mut self.commands)
    }

    /// Sync the visible status values from a snapshot.
    pub fn sync_status(&mut self, snapshot: &EngineSnapshot) {
        self.status = StatusState {
            fps: snapshot.fps,
            frame_ms: snapshot.frame_ms,
            resolution: snapshot.resolution,
            bpm: snapshot.audio.bpm,
            adapter: snapshot.health.adapter.clone(),
            backend: snapshot.health.backend.clone(),
            frames_presented: snapshot.health.frames_presented,
            audio_active: snapshot.health.audio_active,
        };
        self.output_open = snapshot.health.output_open;
        self.p0.output_window_ok = snapshot.health.output_open;
        self.fps_history.push_back(snapshot.fps);
        while self.fps_history.len() > 120 {
            self.fps_history.pop_front();
        }
        if let Some(error) = &snapshot.health.last_error {
            self.last_error = Some(error.clone());
        }
    }

    /// The props of the selected module, if it is still present.
    pub fn selected_props<'a>(&self, snapshot: &'a EngineSnapshot) -> Option<Vec<&'a PropView>> {
        let id = self.selected_module.as_deref()?;
        snapshot
            .module(id)
            .map(|module| module.props.iter().collect())
    }

    /// The selected Group view, if any.
    pub fn selected_group<'a>(&self, snapshot: &'a EngineSnapshot) -> Option<&'a crate::GroupView> {
        let id = self.selected_group.as_deref()?;
        snapshot.groups.iter().find(|group| group.id == id)
    }

    /// Focus the first user Group the first time the snapshot has one, so a
    /// fresh shell opens with that Group's controls visible
    /// (`defaultGroupIsFocusedByDefault.spec.js`).
    ///
    /// Only the first frame with Groups seeds the selection. Clearing it later
    /// (removing the focused Group, or an explicit deselection) must stay
    /// cleared instead of snapping back to the first Group.
    pub fn ensure_default_group_focus(&mut self, snapshot: &EngineSnapshot) {
        if self.initial_focus_done {
            return;
        }
        let Some(group) = snapshot.groups.iter().find(|group| !group.is_gallery()) else {
            // An empty snapshot (startup before Scenes/Groups load) retries on
            // the next frame.
            return;
        };
        if self.selected_group.is_none() {
            self.selected_group = Some(group.id.clone());
            self.selected_module = None;
        }
        self.initial_focus_done = true;
    }
}

/// Gallery section filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GalleryCategory {
    /// Every registered module, regardless of section.
    #[default]
    All,
    Effects,
    TwoDScenes,
    ThreeDScenes,
}

impl GalleryCategory {
    pub const ALL: [Self; 4] = [
        Self::All,
        Self::Effects,
        Self::TwoDScenes,
        Self::ThreeDScenes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Effects => "Effects",
            Self::TwoDScenes => "2D Scenes",
            Self::ThreeDScenes => "3D Scenes",
        }
    }

    /// The legacy Gallery section key. `All` returns an empty string, so this
    /// must never be used for matching; use [`Self::matches`] instead.
    pub fn key(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Effects => "effects",
            Self::TwoDScenes => "2d-scenes",
            Self::ThreeDScenes => "3d-scenes",
        }
    }

    /// Whether a module in `module_category` (from
    /// [`ModuleMetaView::gallery_category`]) belongs to this section.
    pub fn matches(self, module_category: &str) -> bool {
        match self {
            Self::All => true,
            _ => module_category == self.key(),
        }
    }
}

/// The Gallery's visible modules for a category and search text.
///
/// The search matches the module name or author, case-insensitively, and a
/// whitespace-only query behaves like an empty one. There is deliberately no
/// tile cap: the Gallery scrolls instead of truncating the registry.
pub fn filter_gallery_modules<'a>(
    registered: &'a [ModuleMetaView],
    category: GalleryCategory,
    search: &str,
) -> Vec<&'a ModuleMetaView> {
    let filter = search.trim().to_ascii_lowercase();
    registered
        .iter()
        .filter(|module| category.matches(module.gallery_category()))
        .filter(|module| {
            filter.is_empty()
                || module.name.to_ascii_lowercase().contains(&filter)
                || module.author.to_ascii_lowercase().contains(&filter)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_and_commands_drain_in_order() {
        let mut state = UiState::new();
        state.push_action("groups/createGroup", serde_json::json!({}));
        state.push_action("scenes/applyScene", serde_json::json!({"sceneId": "a"}));
        state.queue(UiCommand::OpenOutputWindow);

        let actions = state.take_actions();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].action, "groups/createGroup");
        assert_eq!(actions[1].payload["sceneId"], "a");
        assert!(state.take_actions().is_empty());

        assert_eq!(state.take_commands(), vec![UiCommand::OpenOutputWindow]);
        assert!(state.take_commands().is_empty());
    }

    #[test]
    fn opening_another_link_editor_cancels_a_pending_midi_learn() {
        let mut state = UiState::new();
        let snapshot = EngineSnapshot::default();
        let mut waiting = LinkEditor::audio("input-1", "gain");
        waiting.start_midi_learn(&snapshot);
        state.open_link_editor(waiting);
        assert!(state.actions.is_empty());

        state.open_link_editor(LinkEditor::audio("input-2", "gain"));

        assert_eq!(
            state.take_actions(),
            vec![UiAction::new("midi/cancelLearn", Value::Null)]
        );
        assert_eq!(
            state
                .link_editor
                .as_ref()
                .map(|editor| editor.input_id.as_str()),
            Some("input-2")
        );
    }

    #[test]
    fn preview_registration_flags_the_shared_texture_probe() {
        let mut state = UiState::new();
        state.set_preview(egui::TextureId::User(7), 640, 360);
        assert_eq!(state.preview_size, [640, 360]);
        assert!(state.p0.shared_texture_ok);

        state.clear_preview();
        assert!(state.preview_texture.is_none());
        assert!(!state.p0.shared_texture_ok);
    }

    #[test]
    fn p0_verdict_requires_texture_output_and_frame_rate() {
        let mut p0 = P0State {
            shared_texture_ok: true,
            output_window_ok: true,
            ..Default::default()
        };
        assert!(p0.verdict(60.0, 55.0));
        assert!(!p0.verdict(54.9, 55.0));
        p0.output_window_ok = false;
        assert!(!p0.verdict(120.0, 55.0));
    }

    #[test]
    fn ndi_and_osc_are_the_last_strip_tabs() {
        assert_eq!(
            &SidePanelTab::ALL[SidePanelTab::COUNT - 2..],
            &[SidePanelTab::Ndi, SidePanelTab::Osc]
        );
        for (index, tab) in [(7, SidePanelTab::Ndi), (8, SidePanelTab::Osc)] {
            assert_eq!(tab.index(), index);
            assert_eq!(SidePanelTab::from_key(tab.key()), Some(tab));
        }
        assert_eq!(SidePanelTab::Ndi.title(), "NDI");
        assert_eq!(SidePanelTab::Osc.title(), "OSC");
        assert_eq!(SidePanelTab::Ndi.key(), "ndi");
        assert_eq!(SidePanelTab::Osc.key(), "osc");
    }

    #[test]
    fn the_library_tab_sits_between_media_and_audio() {
        assert_eq!(SidePanelTab::Library.index(), 2);
        assert_eq!(SidePanelTab::Library.key(), "library");
        assert_eq!(SidePanelTab::Library.title(), "Library");
        assert_eq!(
            SidePanelTab::from_key("library"),
            Some(SidePanelTab::Library)
        );
        assert_eq!(
            &SidePanelTab::ALL[1..4],
            &[
                SidePanelTab::Media,
                SidePanelTab::Library,
                SidePanelTab::Audio
            ]
        );
        assert_eq!(SidePanelTab::ALL.len(), 9);
    }

    #[test]
    fn arming_a_library_target_opens_the_matching_folder() {
        let mut state = UiState::new();
        state.enter_library_folder("artist");
        state.arm_library_target(LibraryAssignmentTarget {
            module_id: "module-1".to_string(),
            module_name: "Image".to_string(),
            prop_name: "texture".to_string(),
            kind: LibraryKind::Video,
        });

        assert_eq!(state.side_panel_tab, SidePanelTab::Library);
        assert_eq!(state.library_kind, LibraryKind::Video);
        assert_eq!(
            state.library_folder, "",
            "a kind switch returns to its root"
        );
        assert_eq!(
            state.library_target.as_ref().map(|t| t.describe()),
            Some("Pick videos for Image / texture".to_string())
        );

        state.clear_library_target();
        assert!(state.library_target.is_none());
    }

    #[test]
    fn clicking_a_library_asset_writes_the_armed_texture_prop() {
        let mut state = UiState::new();
        state.arm_library_target(LibraryAssignmentTarget {
            module_id: "module-1".to_string(),
            module_name: "Image".to_string(),
            prop_name: "texture".to_string(),
            kind: LibraryKind::Image,
        });
        let asset = MediaAssetView {
            path: "live/image/loop.png".to_string(),
            kind: LibraryKind::Image,
            name: "loop.png".to_string(),
            ..Default::default()
        };

        assert!(state.assign_library_asset(&asset));
        assert_eq!(
            state.take_actions(),
            vec![texture_prop_action(
                "module-1",
                "texture",
                serde_json::json!({
                    "type": "image",
                    "options": { "path": "live/image/loop.png" },
                })
            )],
            "the exact value the manual path editor writes"
        );
        assert_eq!(
            state.selected_library_asset.as_deref(),
            Some("live/image/loop.png")
        );
        assert!(state.library_target.is_none(), "the pick disarms the prop");

        assert!(
            !state.assign_library_asset(&asset),
            "a second click without a target only selects"
        );
        assert!(state.actions.is_empty());
    }

    #[test]
    fn a_library_asset_of_another_kind_never_fills_a_armed_prop() {
        let mut state = UiState::new();
        state.arm_library_target(LibraryAssignmentTarget {
            module_id: "module-1".to_string(),
            module_name: "Image".to_string(),
            prop_name: "texture".to_string(),
            kind: LibraryKind::Image,
        });
        let video = MediaAssetView {
            path: "live/video/clip.mp4".to_string(),
            kind: LibraryKind::Video,
            name: "clip.mp4".to_string(),
            ..Default::default()
        };

        assert!(!state.assign_library_asset(&video));
        assert!(state.actions.is_empty());
        assert!(
            state.library_target.is_some(),
            "the prop keeps waiting for an image"
        );
    }

    #[test]
    fn library_folder_navigation_resets_on_kind_and_walks_up() {
        let mut state = UiState::new();
        state.open_library_subfolder("artist");
        state.open_library_subfolder("pack");
        assert_eq!(state.library_folder, "artist/pack");

        state.library_up();
        assert_eq!(state.library_folder, "artist");
        state.library_up();
        assert_eq!(state.library_folder, "");

        state.enter_library_folder("/artist/pack/");
        assert_eq!(state.library_folder, "artist/pack");
        state.select_library_kind(LibraryKind::Image);
        assert_eq!(
            state.library_folder, "artist/pack",
            "reselecting the current kind keeps the folder"
        );
        state.select_library_kind(LibraryKind::Palette);
        assert_eq!(state.library_folder, "");
        assert_eq!(state.library_kind, LibraryKind::Palette);
    }

    #[test]
    fn a_tab_is_visible_when_active_in_the_strip_or_placed_elsewhere() {
        let mut state = UiState::new();
        assert!(state.is_tab_visible(SidePanelTab::Inspector));
        assert!(!state.is_tab_visible(SidePanelTab::Ndi));

        state.side_panel_tab = SidePanelTab::Ndi;
        assert!(state.is_tab_visible(SidePanelTab::Ndi));

        state
            .layout
            .tabs
            .set_placement(SidePanelTab::Ndi, crate::layout::TabPlacement::Floating);
        state.side_panel_tab = SidePanelTab::Inspector;
        assert!(
            state.is_tab_visible(SidePanelTab::Ndi),
            "a floating/docked tab is on screen regardless of the strip selection"
        );
        assert!(!state.is_tab_visible(SidePanelTab::Osc));
    }

    #[test]
    fn a_profile_needs_a_name_before_queueing() {
        let mut state = UiState::new();
        state.open_profile_dialog();
        state.profile_name = "   ".to_string();
        state.queue_create_profile();

        assert!(state.commands.is_empty());
        assert!(state.show_profile_dialog, "the modal stays open");
        assert!(state.profile_error.is_some());

        state.profile_name = " live ".to_string();
        state.queue_create_profile();
        assert_eq!(
            state.commands,
            vec![UiCommand::CreateProfile("live".to_string())]
        );
        assert!(!state.show_profile_dialog);
    }

    #[test]
    fn gallery_categories_match_the_legacy_fallbacks() {
        let effects = crate::ModuleMetaView {
            module_type: "isf".to_string(),
            ..Default::default()
        };
        assert_eq!(effects.gallery_category(), "effects");

        let two_d = crate::ModuleMetaView {
            module_type: "2d".to_string(),
            ..Default::default()
        };
        assert_eq!(two_d.gallery_category(), "2d-scenes");

        let explicit = crate::ModuleMetaView {
            module_type: "shader".to_string(),
            category: "3d-scenes".to_string(),
            ..Default::default()
        };
        assert_eq!(explicit.gallery_category(), "3d-scenes");
    }

    #[test]
    fn the_focused_input_title_names_the_owner_and_control() {
        // `ActiveModule.vue`/`Group.vue` dispatch `${name}: ${title}`, so the
        // Input Config window is `Ball: Amount` / `Background: Alpha`.
        let module = crate::ModuleView {
            id: "module-1".to_string(),
            module_name: "Ball".to_string(),
            alpha_input_id: Some("in-alpha".to_string()),
            props: vec![crate::PropView {
                name: "Amount".to_string(),
                input_id: Some("in-amount".to_string()),
                components: vec![crate::PropComponentView {
                    key: "g".to_string(),
                    input_id: "in-fill-g".to_string(),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let group = crate::GroupView {
            name: "Background".to_string(),
            alpha_input_id: Some("in-group-alpha".to_string()),
            ..Default::default()
        };
        let snapshot = EngineSnapshot {
            modules: vec![module],
            groups: vec![group],
            ..Default::default()
        };

        assert_eq!(
            focused_input_title("in-amount", "Amount", &snapshot),
            "Ball: Amount"
        );
        assert_eq!(
            focused_input_title("in-fill-g", "fillColor.g", &snapshot),
            "Ball: fillColor.g",
            "component sub-inputs keep their dotted label"
        );
        assert_eq!(
            focused_input_title("in-alpha", "Alpha", &snapshot),
            "Ball: Alpha"
        );
        assert_eq!(
            focused_input_title("in-group-alpha", "Group alpha", &snapshot),
            "Background: Alpha",
            "Group controls use the Group name and a capitalised control"
        );
        assert_eq!(
            focused_input_title("unowned", "gain", &snapshot),
            "gain",
            "an input with no owner keeps the bare control label"
        );
    }

    #[test]
    fn the_startup_focus_runs_once() {
        let mut state = UiState::new();
        let snapshot = EngineSnapshot {
            groups: vec![
                crate::GroupView {
                    id: "gallery".to_string(),
                    name: crate::model::GALLERY_GROUP_NAME.to_string(),
                    ..Default::default()
                },
                crate::GroupView {
                    id: "group-bg".to_string(),
                    name: "Background".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        state.ensure_default_group_focus(&snapshot);
        assert_eq!(
            state.selected_group.as_deref(),
            Some("group-bg"),
            "the internal Gallery Group is skipped"
        );

        // Clearing the selection must not snap back on the next frame.
        state.selected_group = None;
        state.ensure_default_group_focus(&snapshot);
        assert!(state.selected_group.is_none());

        // An empty first frame retries once Groups exist.
        let mut state = UiState::new();
        state.ensure_default_group_focus(&EngineSnapshot::default());
        assert!(state.selected_group.is_none());
        state.ensure_default_group_focus(&snapshot);
        assert_eq!(state.selected_group.as_deref(), Some("group-bg"));
    }
}
