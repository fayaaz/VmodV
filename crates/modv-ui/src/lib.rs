//! Native egui UI for modV.
//!
//! The UI is deliberately engine-agnostic: it renders an [`EngineSnapshot`]
//! (a once-per-frame read model built by `modv-app`) and emits [`UiAction`]s
//! that the app dispatches into the in-process engine store. There is no Vuex
//! mirror, no IPC and no JavaScript in this crate.
//!
//! P0 is the framework spike: a shell with a Scenes-grid cell, a RangeControl
//! and a live GPU texture registered from the engine compositor. The module
//! layout follows `docs/rust-full-rewrite.md` (`modv-ui`):
//!
//! - [`model`] — engine read models shared with `modv-app`
//! - [`state`] — UI-local state and the action/command queues
//! - [`shell`] — window chrome, menus, status bar and panel composition
//! - [`widgets`] — the prop-driven control set
//! - [`panels`] — dockable panel implementations
//! - [`theme`] — the black/neon-green theme and type scale
//! - [`fonts`] — the bundled IBM Plex faces

#![forbid(unsafe_code)]

pub mod file_dialog;
pub mod fonts;
pub mod gallery_drag;
pub mod group_drag;
pub mod layout;
pub mod library_drag;
pub mod model;
pub mod module_drag;
pub mod panels;
pub mod shell;
pub mod state;
pub mod tab_drag;
pub mod theme;
pub mod widgets;

pub use file_dialog::{FileDialogAction, FileDialogMode, FileDialogState};
pub use layout::{LayoutState, TabLayout, TabPlacement, WindowBounds};
pub use library_drag::{
    MediaLibraryDrag, USE_FROM_LIBRARY_LABEL, texture_value_for_asset, texture_value_for_control,
};
pub use model::{
    AudioDeviceView, AudioSelectionView, AudioView, COMPOSITE_OPERATIONS, CameraModeView,
    CameraView, DEFAULT_PROFILE_NAME, EngineHealth, EngineSnapshot, ExpressionView, GroupView,
    InputLinkView, LibraryKind, MediaAction, MediaAssetView, MediaLibraryView, MediaSnapshot,
    MediaSourceView, MidiDeviceView, MidiLearnArmView, MidiLearnResultView, MidiSnapshot,
    ModuleMetaView, ModuleView, PERFORMANCE_ORDER_IN_ORDER, PERFORMANCE_ORDER_RANDOM,
    PERFORMANCE_ORDERS, PerformanceView, PluginInfoView, PluginPropKindView, PluginPropOptionView,
    PluginPropView, PluginStatus, PluginView, PreviewRequest, PreviewTexture, ProjectView,
    PropComponentView, PropEnumMember, PropView, RowMonitorView, SceneCellView, SceneView,
    ScreenCaptureView, ScreenShareKindView, ScreenSourceView, TextureSourceView, TimingUnit,
    TweenView, VideoSourceProblemView, VideoTransportAction, VideoTransportView, bar_seconds,
    bpm_with_rate, default_camera_mode, default_timing_values, effective_bpm, fps_options,
    gallery_preview_key, mode_for_resolution, normalise_video_path, normalize_performance_order,
    performance_order_label, plugin_color_value, sample_rate_label, scene_preview_key, timing_hint,
    timing_seconds_value, timing_value_seconds, un_premultiply_rgba, unique_resolutions,
};
pub use state::{
    AUDIO_FEATURES, DrawCurveEditor, LibraryAssignmentTarget, LinkEditor, MAX_SMOOTHING,
    SMOOTHING_STEP, SidePanelTab, TweenCurveEditor, TweenHandleDrag, UiAction, UiCommand, UiState,
    mint_smoothing_id, texture_prop_action,
};
