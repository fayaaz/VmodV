//! UI-local state and the action/command queues.
//!
//! Immediate mode keeps the UI stateless per frame except for durable
//! interaction state (selection, focus, drag sources) and the command queues
//! the app drains after drawing.

use std::collections::VecDeque;

use serde_json::Value;

use crate::model::{EngineSnapshot, MediaAction, ModuleMetaView, PreviewRequest, PropView};
use crate::panels::scene_grid::SceneGridState;
use crate::widgets::RangeControl;

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
}

/// An application command that is not an engine action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiCommand {
    Quit,
    OpenOutputWindow,
    CloseOutputWindow,
    ToggleOutputFullscreen,
    SaveSnapshot,
    SavePreset,
    FlushLogs,
    /// Switch native audio capture live. `source` is `pipewire` or `input`;
    /// an `input` with no `device` selects the host default input.
    SetAudioSource {
        source: String,
        device: Option<String>,
    },
    /// Re-enumerate cpal input devices (hot-plug).
    RefreshAudioDevices,
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
    pub output_open: bool,
    pub show_p0: bool,
    pub show_about: bool,
    pub show_shortcuts: bool,
    pub search_focus: bool,
    /// One-shot focus request for the Ctrl+K palette (separate from the
    /// Gallery search field).
    pub palette_focus: bool,
    pub link_editor: Option<LinkEditor>,
    pub show_bpm: bool,
    pub show_search: bool,
    pub command_query: String,
    pub command_selection: usize,
    pub bpm_source: String,
    pub tap_times: Vec<std::time::Instant>,
    pub side_panel_tab: SidePanelTab,
    /// MediaHub operations drained by the shell after drawing.
    pub media_actions: Vec<MediaAction>,
    pub p0: P0State,
    pub inspector_probe: RangeControl,
    pub inspector_probe_value: f64,
    /// The most recent engine error rendered in the status bar/toast list.
    pub last_error: Option<String>,
    pub status: StatusState,
    /// Rolling frame-rate history for the status sparkline.
    pub fps_history: VecDeque<f32>,
}

/// Which panel the right side shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidePanelTab {
    #[default]
    Inspector,
    Media,
    Audio,
    Info,
    Plugins,
    Midi,
}

/// The focused-input link editor's draft values.
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
            exists: false,
            error: None,
        }
    }

    pub fn from_link(
        input_id: impl Into<String>,
        title: impl Into<String>,
        link: &crate::InputLinkView,
    ) -> Self {
        Self {
            input_id: input_id.into(),
            title: title.into(),
            kind: link.kind.clone(),
            source: link.source.clone().unwrap_or_else(|| "state".to_string()),
            location: link.location.clone().unwrap_or_default(),
            args: link
                .args
                .as_ref()
                .map(|args| args.to_string())
                .unwrap_or_else(|| "[]".to_string()),
            min: link.min.unwrap_or(0.0),
            max: link.max.unwrap_or(1.0),
            exists: true,
            error: None,
        }
    }

    /// The audio feature named by the getter args, if any.
    pub fn audio_feature(&self) -> String {
        serde_json::from_str::<Value>(&self.args)
            .ok()
            .and_then(|value| value.as_array()?.first()?.as_str().map(str::to_string))
            .unwrap_or_else(|| "bass".to_string())
    }

    pub fn set_audio_feature(&mut self, feature: &str) {
        self.kind = "getter".to_string();
        self.source = "meyda".to_string();
        self.location = "meyda/getFeature".to_string();
        self.args = serde_json::json!([feature]).to_string();
    }

    pub fn parsed_args(&self) -> Result<Value, String> {
        if self.kind != "getter" {
            return Ok(Value::Null);
        }
        serde_json::from_str::<Value>(&self.args)
            .map_err(|error| format!("args must be a JSON array: {error}"))
    }
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
            output_open: false,
            show_p0: true,
            show_about: false,
            show_shortcuts: false,
            search_focus: false,
            palette_focus: false,
            link_editor: None,
            show_bpm: false,
            show_search: false,
            command_query: String::new(),
            command_selection: 0,
            bpm_source: "beatdetektor".to_string(),
            tap_times: Vec::new(),
            side_panel_tab: SidePanelTab::Inspector,
            media_actions: Vec::new(),
            p0: P0State::default(),
            inspector_probe,
            inspector_probe_value: 0.5,
            last_error: None,
            status: StatusState::default(),
            fps_history: VecDeque::new(),
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
}
