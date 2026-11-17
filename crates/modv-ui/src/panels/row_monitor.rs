//! The Scenes-grid row monitor policy: which row's live-output thumbnail
//! refreshes at a given frame, and how a row that is not playing is drawn.
//!
//! The monitor's pixel source lives in `modv-app`: the compositor keeps each
//! live Group's rendered texture after composition, and the shell copies one
//! row's texture into a per-group snapshot at the cadence decided here
//! (option (a) of the design: a plain GPU-GPU copy, no readback, no extra
//! pass in the production frame graph, no isolated state materialised). The
//! policy itself is pure so it can be pinned by unit tests:
//!
//! - [`row_monitor_refresh_row`] — the one row (at most) that refreshes on a
//!   frame, keeping every row inside a slow ~3 fps band no matter how many
//!   rows the grid has;
//! - [`row_monitor_display`] — full alpha while performance mode runs,
//!   dimmed to [`INACTIVE_ROW_MONITOR_ALPHA`] when it is off, and the
//!   "no layer" placeholder for a row with no live Group.

/// Frames between two refreshes of the same row when it is the only
/// monitored row — ~3 refreshes per second at the engine's 60 fps target.
pub const ROW_MONITOR_REFRESH_BUDGET: u64 = 20;

/// Alpha of a row monitor while the row is live but performance mode is off.
pub const INACTIVE_ROW_MONITOR_ALPHA: f32 = 0.35;

/// Alpha of a row monitor while performance mode is running.
pub const ACTIVE_ROW_MONITOR_ALPHA: f32 = 1.0;

/// How one row's monitor is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowMonitorDisplay {
    /// Draw the live thumbnail at the given alpha.
    Monitor { alpha: f32 },
    /// The row has no live Group: draw the subtle "no layer" placeholder.
    NoLayer,
}

/// Frames between two refresh slots.
///
/// `row_count` rows share the one-refresh-per-frame budget; spacing the slots
/// by `budget / row_count` (at least one frame) keeps every row inside the
/// slow-cadence band: 6 rows refresh each ~18 frames (~3.3 fps), and a grid
/// so wide it exceeds the budget degrades to one row per frame rather than
/// exceeding it.
pub fn row_monitor_stride(row_count: usize) -> u64 {
    if row_count == 0 {
        return ROW_MONITOR_REFRESH_BUDGET;
    }
    (ROW_MONITOR_REFRESH_BUDGET / row_count as u64).max(1)
}

/// The row (as an index into the grid's sorted list of rows that have a live
/// Group) whose monitor refreshes on `frame_index`, if any.
///
/// At most one row refreshes per frame; each row refreshes once per
/// `stride * row_count` frames.
pub fn row_monitor_refresh_row(frame_index: u64, row_count: usize) -> Option<usize> {
    if row_count == 0 {
        return None;
    }
    let stride = row_monitor_stride(row_count);
    let cycle = stride * row_count as u64;
    let phase = frame_index % cycle;
    (phase.is_multiple_of(stride)).then(|| (phase / stride) as usize)
}

/// How the row's monitor is drawn given whether a live Group renders the row
/// and whether performance mode is running.
pub fn row_monitor_display(live_group: bool, performance_enabled: bool) -> RowMonitorDisplay {
    match (live_group, performance_enabled) {
        (false, _) => RowMonitorDisplay::NoLayer,
        (true, true) => RowMonitorDisplay::Monitor {
            alpha: ACTIVE_ROW_MONITOR_ALPHA,
        },
        (true, false) => RowMonitorDisplay::Monitor {
            alpha: INACTIVE_ROW_MONITOR_ALPHA,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rowless_grid_refreshes_nothing() {
        for frame in 0..50u64 {
            assert_eq!(
                row_monitor_refresh_row(frame, 0),
                None,
                "frame {frame} must not refresh a monitor when no row is live"
            );
        }
    }

    #[test]
    fn every_row_refreshes_once_per_cycle_and_never_two_per_frame() {
        for row_count in [1, 2, 3, 6, 12, 25] {
            let stride = row_monitor_stride(row_count);
            let cycle = stride * row_count as u64;
            let mut refreshes_per_row = vec![0u32; row_count];
            let mut last_refresh: Vec<Option<u64>> = vec![None; row_count];

            for frame in 0..(cycle * 3) {
                if let Some(row) = row_monitor_refresh_row(frame, row_count) {
                    assert!(
                        row < row_count,
                        "row_count {row_count}: row {row} is out of range"
                    );
                    refreshes_per_row[row] += 1;
                    if let Some(previous) = last_refresh[row] {
                        assert_eq!(
                            frame - previous,
                            cycle,
                            "row_count {row_count}: row {row} refreshed early at {frame}"
                        );
                    }
                    last_refresh[row] = Some(frame);
                }
            }

            assert!(
                refreshes_per_row.iter().all(|count| *count == 3),
                "row_count {row_count}: every row refreshes once per cycle, got \
                 {refreshes_per_row:?}"
            );
        }
    }

    #[test]
    fn the_cadence_stays_slow_no_matter_how_many_rows() {
        // With the one-refresh-per-frame cap, a row's gap is at most
        // max(budget, row_count) frames: 3 fps for a handful of rows and
        // still ≥ ~2.4 fps for a 25-row grid.
        for row_count in 1..=32 {
            let stride = row_monitor_stride(row_count);
            assert!(stride >= 1, "stride must be at least one frame");
            let gap = stride * row_count as u64;
            assert!(
                gap <= ROW_MONITOR_REFRESH_BUDGET.max(row_count as u64),
                "row_count {row_count}: gap {gap} leaves the slow-cadence band"
            );
            assert!(
                gap >= ROW_MONITOR_REFRESH_BUDGET / 2,
                "row_count {row_count}: gap {gap} is faster than the budget allows"
            );
        }
    }

    #[test]
    fn rows_without_a_group_show_the_no_layer_placeholder() {
        assert_eq!(row_monitor_display(false, true), RowMonitorDisplay::NoLayer);
        assert_eq!(
            row_monitor_display(false, false),
            RowMonitorDisplay::NoLayer
        );
    }

    #[test]
    fn a_live_row_monitor_is_dimmed_while_performance_mode_is_off() {
        assert_eq!(
            row_monitor_display(true, false),
            RowMonitorDisplay::Monitor {
                alpha: INACTIVE_ROW_MONITOR_ALPHA
            }
        );
    }

    #[test]
    fn a_live_row_monitor_is_full_alpha_while_performing() {
        assert_eq!(
            row_monitor_display(true, true),
            RowMonitorDisplay::Monitor {
                alpha: ACTIVE_ROW_MONITOR_ALPHA
            }
        );
    }
}
