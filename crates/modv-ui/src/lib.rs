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
//! - [`theme`] — the dark theme ported from the legacy CSS

#![forbid(unsafe_code)]

pub mod model;
pub mod panels;
pub mod shell;
pub mod state;
pub mod theme;
pub mod widgets;

pub use model::{
    AudioView, EngineHealth, EngineSnapshot, GroupView, ModuleMetaView, ModuleView,
    PerformanceView, PropView, SceneCellView, SceneView,
};
pub use state::{UiAction, UiCommand, UiState};
