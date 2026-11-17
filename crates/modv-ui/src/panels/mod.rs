//! Dockable panels.
//!
//! P0 ships the Scenes-grid cell/grid probe; P4 adds the remaining panels
//! (Gallery, Groups, Input Config, device panels, status, search, plugins).

pub mod scene_grid;

pub use scene_grid::{CellRef, SceneGridState, draw_scene_grid};
