//! Golden coverage for the Scenes grid's per-row layer monitor.
//!
//! Row 0 carries a live monitor texture (letterboxed into the gutter),
//! row 1 has a live Group whose first refresh has not landed (an empty
//! plate, not a "no layer" claim) and row 2 has no Group at all (the
//! subtle placeholder). Regenerate with `UPDATE_SNAPSHOTS=1`.

use egui_kittest::Harness;
use modv_ui::model::{PreviewTexture, RowMonitorView};
use modv_ui::{EngineSnapshot, PerformanceView, SceneCellView, SceneView, UiState};

/// A 16 × 9 test card with a visible diagonal so the thumbnail's
/// letterboxing is checkable in the golden.
fn test_card() -> egui::ColorImage {
    let mut pixels = Vec::with_capacity(16 * 9 * 3);
    for y in 0..9u8 {
        for x in 0..16u8 {
            let lit = if x * 9 >= y * 16 { 230 } else { 40 };
            pixels.extend_from_slice(&[lit, 120 - y * 6, 200 - x * 8]);
        }
    }
    egui::ColorImage::from_rgb([16, 9], &pixels)
}

fn snapshot() -> EngineSnapshot {
    let scene = SceneView {
        id: "scene-1".to_string(),
        name: "Lone Chrome".to_string(),
        enabled: true,
        timing: Some(8.0),
        cells: vec![
            SceneCellView {
                row: 0,
                module_names: vec!["Bands".to_string()],
                disabled: false,
            },
            SceneCellView {
                row: 1,
                module_names: vec!["Chromatic Line".to_string()],
                disabled: true,
            },
            SceneCellView {
                row: 2,
                module_names: vec![],
                disabled: false,
            },
        ],
    };
    EngineSnapshot {
        scenes: vec![scene],
        scene_rows: vec![
            "Background".to_string(),
            "Foreground".to_string(),
            "Effects".to_string(),
        ],
        performance: PerformanceView {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn scene_grid_row_monitor_golden() {
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size([560.0, 440.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let monitor = ui.ctx().load_texture(
                    "row-monitor-frame",
                    test_card(),
                    egui::TextureOptions::LINEAR,
                );
                let mut snapshot = snapshot();
                snapshot.row_monitors.insert(
                    0,
                    RowMonitorView {
                        group_id: "group-0".to_string(),
                        texture: Some(PreviewTexture {
                            id: monitor.id(),
                            width: 16,
                            height: 9,
                        }),
                    },
                );
                snapshot.row_monitors.insert(
                    1,
                    RowMonitorView {
                        group_id: "group-1".to_string(),
                        texture: None,
                    },
                );
                modv_ui::panels::draw_scene_grid(
                    ui,
                    &snapshot,
                    &mut state.scene_grid,
                    None,
                    &mut state.actions,
                    &mut state.layout,
                );
            },
            state,
        );
    harness.run();
    harness.snapshot("scene_grid_row_monitor");
}

#[test]
fn scene_grid_row_monitor_dims_while_performance_mode_is_off() {
    // The same layout with performance mode stopped: the live monitor draws
    // at the inactive dim alpha. The tint difference is tiny in a golden, so
    // this golden is generated once and guards the drawing path against
    // panics/layout shifts when the dim policy applies.
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size([560.0, 440.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let monitor = ui.ctx().load_texture(
                    "row-monitor-frame",
                    test_card(),
                    egui::TextureOptions::LINEAR,
                );
                let mut snapshot = snapshot();
                snapshot.performance.enabled = false;
                snapshot.row_monitors.insert(
                    0,
                    RowMonitorView {
                        group_id: "group-0".to_string(),
                        texture: Some(PreviewTexture {
                            id: monitor.id(),
                            width: 16,
                            height: 9,
                        }),
                    },
                );
                modv_ui::panels::draw_scene_grid(
                    ui,
                    &snapshot,
                    &mut state.scene_grid,
                    None,
                    &mut state.actions,
                    &mut state.layout,
                );
            },
            state,
        );
    harness.run();
    harness.snapshot("scene_grid_row_monitor_dimmed");
}
