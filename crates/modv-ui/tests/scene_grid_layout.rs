//! Rect-based coverage for the Scenes grid's fixed row pitch and pinned layer
//! gutter.
//!
//! The grid used to lay rows out with the ordinary vertical cursor, so the
//! per-cell enable dot (a child widget placed inside the cell) rewound each
//! column's cursor: rows overlapped and every Scene column drifted by a
//! different amount. The gutter was also part of the horizontally scrolling
//! content, so it slid away with the Scene columns.
//!
//! These tests read the real widget rects from the accessibility tree (the
//! cells and layer labels are labelled buttons) instead of goldens, and drive
//! the scroll areas with wheel events.

use egui::{Pos2, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, PerformanceView, SceneCellView, SceneView, UiState};

const ROW_NAMES: [&str; 5] = ["WEBCAM", "Background", "Foreground", "Effects", "Master"];

/// The Scene columns, deliberately mixing empty, populated and disabled cells
/// in every row so a content-dependent pitch shows up immediately.
const SCENES: [(&str, &str); 4] = [
    ("scene-alpha", "Alpha"),
    ("scene-beta", "Beta"),
    ("scene-gamma", "Gamma"),
    ("scene-delta", "Delta"),
];

const CELL_HEIGHT: f32 = 76.0;
/// `CELL_SIZE.y` plus the fixed gap between rows.
const ROW_PITCH: f32 = 81.0;
/// Mirrors `ROW_LABEL_WIDTH` in `src/panels/scene_grid.rs`.
const LABEL_WIDTH: f32 = 120.0;
const EPSILON: f32 = 0.1;

fn populated(row: usize, names: &[&str], disabled: bool) -> SceneCellView {
    SceneCellView {
        row,
        module_names: names.iter().map(|name| (*name).to_string()).collect(),
        disabled,
    }
}

fn empty(row: usize) -> SceneCellView {
    SceneCellView {
        row,
        module_names: Vec::new(),
        disabled: false,
    }
}

fn scene(id: &str, name: &str, timing: Option<(f64, f64)>, cells: Vec<SceneCellView>) -> SceneView {
    SceneView {
        id: id.to_string(),
        name: name.to_string(),
        enabled: true,
        timing,
        cells,
    }
}

fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        scenes: vec![
            scene(
                "scene-alpha",
                "Alpha",
                Some((4.0, 8.0)),
                vec![
                    populated(0, &["Bands"], false),
                    empty(1),
                    populated(2, &["Chromatic Line", "Kaleidoscope"], false),
                    empty(3),
                    populated(4, &["Camera"], true),
                ],
            ),
            scene(
                "scene-beta",
                "Beta",
                None,
                vec![
                    empty(0),
                    populated(1, &["Ferrofluid"], true),
                    empty(2),
                    populated(3, &["Bands"], false),
                    empty(4),
                ],
            ),
            scene(
                "scene-gamma",
                "Gamma",
                None,
                vec![
                    populated(0, &["Bands"], false),
                    populated(1, &["Bands", "Chromatic Line", "Kaleidoscope"], false),
                    empty(2),
                    empty(3),
                    populated(4, &["Bands"], false),
                ],
            ),
            scene(
                "scene-delta",
                "Delta",
                None,
                vec![empty(0), empty(1), empty(2), empty(3), empty(4)],
            ),
        ],
        scene_rows: ROW_NAMES.iter().map(|name| name.to_string()).collect(),
        performance: PerformanceView {
            enabled: true,
            current_scene_id: Some("scene-alpha".to_string()),
            min_value: 8.0,
            max_value: 24.0,
            transition_seconds: 2.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn harness(size: [f32; 2]) -> Harness<'static, UiState> {
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size(size)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let snapshot = snapshot();
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
    modv_ui::theme::apply(&harness.ctx);
    harness.run();
    harness
}

fn cell_rect(harness: &Harness<'_, UiState>, scene: &str, row: usize) -> Rect {
    harness.get_by_label(&format!("{scene} Group {row}")).rect()
}

fn layer_rect(harness: &Harness<'_, UiState>, row: usize) -> Rect {
    harness
        .get_by_label(&format!(
            "Group {row}: {}",
            ROW_NAMES[row.saturating_sub(1)]
        ))
        .rect()
}

fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "{what}: expected {expected}, got {actual}"
    );
}

/// Send a mouse-wheel event with the pointer over `position` and let the
/// scroll area settle (egui smooths wheel deltas over several frames).
fn wheel(harness: &mut Harness<'static, UiState>, position: Pos2, delta: Vec2) {
    harness.hover_at(position);
    harness.run();
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
}

#[test]
fn every_row_has_one_fixed_pitch_in_the_gutter_and_every_scene_column() {
    // Tall enough that the whole grid is on screen: clipped widgets are still
    // in the accessibility tree, but keeping them visible makes failures read
    // as layout errors rather than culling.
    let harness = harness([640.0, 620.0]);
    let layers_header = harness.get_by_label("Groups").rect();

    let mut previous: Option<(f32, Vec<f32>)> = None;
    let mut column_lefts: Vec<f32> = Vec::new();

    for (index, row_name) in ROW_NAMES.iter().enumerate() {
        let row = index + 1;
        let layer = layer_rect(&harness, row);
        assert_close(
            layer.width(),
            LABEL_WIDTH,
            &format!("layer row {row} width"),
        );
        assert_close(
            layer.height(),
            CELL_HEIGHT,
            &format!("layer row {row} height"),
        );
        if index == 0 {
            assert!(
                layer.top() >= layers_header.bottom(),
                "the first row starts below the Layers header: {layer:?} vs {layers_header:?}"
            );
        }

        let mut tops = Vec::with_capacity(SCENES.len());
        for (column, (_, scene)) in SCENES.iter().enumerate() {
            let cell = cell_rect(&harness, scene, row);
            assert_close(
                cell.height(),
                CELL_HEIGHT,
                &format!("{scene} Group {row} height (Group name {row_name})"),
            );
            assert_close(
                cell.top(),
                layer.top(),
                &format!("{scene} row {row} top vs the layer gutter"),
            );
            if index == 0 {
                assert!(
                    cell.left() > layer.right(),
                    "{scene} must start to the right of the layer gutter: cell {cell:?}, gutter {layer:?}"
                );
                column_lefts.push(cell.left());
            } else {
                assert_close(
                    cell.left(),
                    column_lefts[column],
                    &format!("{scene} row {row} left stays in its column"),
                );
            }
            tops.push(cell.top());
        }

        if let Some((previous_top, previous_tops)) = &previous {
            assert_close(
                layer.top() - previous_top,
                ROW_PITCH,
                &format!("layer row {row} pitch"),
            );
            for (column, (_, scene)) in SCENES.iter().enumerate() {
                assert_close(
                    tops[column] - previous_tops[column],
                    ROW_PITCH,
                    &format!("{scene} row {row} pitch"),
                );
            }
        }

        previous = Some((layer.top(), tops));
    }

    // The gutter and every Scene column share the same top, so this also
    // proves populated, empty and disabled cells all use that pitch.
    let gutter_top = layer_rect(&harness, 1).top();
    for (_, scene) in SCENES {
        assert_close(
            cell_rect(&harness, scene, 1).top(),
            gutter_top,
            &format!("{scene} starts on the gutter's first row"),
        );
    }
}

#[test]
fn the_layer_gutter_stays_pinned_while_the_scene_columns_scroll_sideways() {
    let mut harness = harness([640.0, 420.0]);

    let cell_before = cell_rect(&harness, "Alpha", 2);
    let header_before = harness.get_by_label("Alpha (applied Scene)").rect();
    let gutter_before = layer_rect(&harness, 2);

    wheel(&mut harness, cell_before.center(), egui::vec2(-140.0, 0.0));

    let cell_after = cell_rect(&harness, "Alpha", 2);
    let header_after = harness.get_by_label("Alpha (applied Scene)").rect();
    let gutter_after = layer_rect(&harness, 2);

    assert_close(
        gutter_after.left(),
        gutter_before.left(),
        "the layer gutter is pinned to the viewport's left edge",
    );
    assert_close(
        gutter_after.top(),
        gutter_before.top(),
        "a sideways scroll must not move rows vertically",
    );
    assert!(
        cell_after.left() < cell_before.left() - 10.0,
        "the Scene columns move left: {cell_before:?} -> {cell_after:?}"
    );
    assert_close(
        header_after.left() - cell_after.left(),
        header_before.left() - cell_before.left(),
        "the Scene header keeps its column's x after scrolling",
    );
    assert_close(
        cell_after.top(),
        gutter_after.top(),
        "the pinned gutter row stays on its Scene row",
    );
}

#[test]
fn the_layer_gutter_and_scene_cells_share_the_vertical_scroll_offset() {
    let mut harness = harness([640.0, 360.0]);

    let label_before = layer_rect(&harness, 2);
    let cell_before = cell_rect(&harness, "Alpha", 2);
    assert_close(
        label_before.top(),
        cell_before.top(),
        "the gutter and the cell are aligned before scrolling",
    );

    wheel(&mut harness, cell_before.center(), egui::vec2(0.0, -90.0));

    let label_after = layer_rect(&harness, 2);
    let cell_after = cell_rect(&harness, "Alpha", 2);
    let label_dy = label_after.top() - label_before.top();
    let cell_dy = cell_after.top() - cell_before.top();

    assert!(
        cell_dy < -10.0,
        "the grid scrolled down: cell moved {cell_dy} px"
    );
    assert_close(
        label_dy,
        cell_dy,
        "the gutter shares the Scene columns' vertical offset",
    );
    assert_close(
        label_after.top(),
        cell_after.top(),
        "the row is still aligned after scrolling",
    );
}
