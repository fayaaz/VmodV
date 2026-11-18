//! Dockable panels.
//!
//! P0 ships the Scenes-grid cell/grid probe; P4 adds the remaining panels
//! (Gallery, Groups, Input Config, device panels, status, search, plugins).

pub mod row_monitor;
pub mod scene_grid;

pub use row_monitor::{
    RowMonitorDisplay, row_monitor_display, row_monitor_refresh_row, row_monitor_stride,
};
pub use scene_grid::{
    CellRef, SceneGridState, draw_performance_bar, draw_scene_grid, draw_timing_controls,
};
