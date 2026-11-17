//! UI-local state and the action/command queues.
//!
//! Immediate mode keeps the UI stateless per frame except for durable
//! interaction state (selection, focus, drag sources) and the command queues
//! the app drains after drawing.

use serde_json::Value;

use crate::model::{EngineSnapshot, PropView};
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
    FlushLogs,
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
    pub gallery_category: GalleryCategory,
    pub search: String,
    pub scene_grid: SceneGridState,
    pub actions: Vec<UiAction>,
    pub commands: Vec<UiCommand>,
    pub preview_texture: Option<egui::TextureId>,
    pub preview_size: [usize; 2],
    pub output_open: bool,
    pub show_p0: bool,
    pub show_about: bool,
    pub p0: P0State,
    pub inspector_probe: RangeControl,
    pub inspector_probe_value: f64,
    /// The most recent engine error rendered in the status bar/toast list.
    pub last_error: Option<String>,
    pub status: StatusState,
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
            gallery_category: GalleryCategory::Effects,
            search: String::new(),
            scene_grid: SceneGridState::default(),
            actions: Vec::new(),
            commands: Vec::new(),
            preview_texture: None,
            preview_size: [0, 0],
            output_open: false,
            show_p0: true,
            show_about: false,
            p0: P0State::default(),
            inspector_probe,
            inspector_probe_value: 0.5,
            last_error: None,
            status: StatusState::default(),
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
    #[default]
    Effects,
    TwoDScenes,
    ThreeDScenes,
}

impl GalleryCategory {
    pub const ALL: [Self; 3] = [Self::Effects, Self::TwoDScenes, Self::ThreeDScenes];

    pub fn label(self) -> &'static str {
        match self {
            Self::Effects => "Effects",
            Self::TwoDScenes => "2D Scenes",
            Self::ThreeDScenes => "3D Scenes",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Effects => "effects",
            Self::TwoDScenes => "2d-scenes",
            Self::ThreeDScenes => "3d-scenes",
        }
    }
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
