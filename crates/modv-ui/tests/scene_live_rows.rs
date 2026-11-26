//! Per-row live Scene marking in the Scenes grid.
//!
//! Performance mode advances each row independently (`scenes/SET_ACTIVE_ROWS`
//! maps a row to its live Scene). The grid used to highlight every cell in the
//! applied Scene's column, which implied a single whole Scene was live even
//! after the rows diverged. These tests pin the pure cell decision, the
//! accessible labels kittest can assert without pixels, and one golden of the
//! divergent state.

use std::collections::BTreeMap;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::panels::scene_grid::{SceneCellHighlight, scene_cell_highlight};
use modv_ui::{EngineSnapshot, PerformanceView, SceneCellView, SceneView, UiState};

const ROW_NAMES: [&str; 3] = ["Background", "Foreground", "Effects"];

fn populated(row: usize, name: &str) -> SceneCellView {
    SceneCellView {
        row,
        module_names: vec![name.to_string()],
        disabled: false,
    }
}

fn empty(row: usize) -> SceneCellView {
    SceneCellView {
        row,
        module_names: Vec::new(),
        disabled: false,
    }
}

fn scene(id: &str, name: &str, cells: Vec<SceneCellView>) -> SceneView {
    SceneView {
        id: id.to_string(),
        name: name.to_string(),
        enabled: true,
        timing: None,
        cells,
    }
}

/// Three rows: row 0 is live on Scene A, row 1 on Scene B and row 2 has no
/// live entry. Scene A is also the applied Scene, so the old whole-column
/// highlight would incorrectly light all three of its cells.
fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        scenes: vec![
            scene(
                "scene-a",
                "Aurora",
                vec![populated(0, "Bands"), populated(1, "Rings"), empty(2)],
            ),
            scene(
                "scene-b",
                "Basalt",
                vec![
                    populated(0, "Bands"),
                    populated(1, "Orbs"),
                    populated(2, "Smoke"),
                ],
            ),
            scene(
                "scene-c",
                "Cinder",
                vec![empty(0), empty(1), populated(2, "Smoke")],
            ),
        ],
        scene_rows: ROW_NAMES.iter().map(|name| name.to_string()).collect(),
        performance: PerformanceView {
            enabled: true,
            current_scene_id: Some("scene-a".to_string()),
            min_value: 8.0,
            max_value: 24.0,
            transition_seconds: 2.0,
            ..Default::default()
        },
        active_scene_rows: BTreeMap::from([(0, "scene-a".to_string()), (1, "scene-b".to_string())]),
        ..Default::default()
    }
}

fn harness(size: [f32; 2]) -> Harness<'static, UiState> {
    harness_with_performance(size, true)
}

fn harness_with_performance(
    size: [f32; 2],
    performance_enabled: bool,
) -> Harness<'static, UiState> {
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size(size)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let mut snapshot = snapshot();
                snapshot.performance.enabled = performance_enabled;
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

fn live_rows(entries: &[(usize, &str)]) -> BTreeMap<usize, String> {
    entries
        .iter()
        .map(|(row, scene)| (*row, (*scene).to_string()))
        .collect()
}

#[test]
fn only_each_rows_live_cell_is_marked() {
    let harness = harness([700.0, 480.0]);

    // Row 0's live Scene is Aurora and row 1's is Basalt.
    harness.get_by_label("LIVE Aurora Group 1");
    harness.get_by_label("LIVE Basalt Group 2");

    // Scene A is the applied Scene, but only its row 0 cell is live.
    assert!(
        harness.query_by_label("LIVE Aurora Group 2").is_none(),
        "the applied Scene's other rows must not be marked live"
    );
    assert!(
        harness.query_by_label("LIVE Aurora Group 3").is_none(),
        "the applied Scene's other rows must not be marked live"
    );
    assert!(
        harness.query_by_label("LIVE Basalt Group 1").is_none(),
        "Basalt is live only in Group 1"
    );
    assert!(
        harness.query_by_label("LIVE Cinder Group 3").is_none(),
        "row 2 has no live entry"
    );

    assert_eq!(
        harness.get_all_by_label_contains("LIVE ").count(),
        2,
        "exactly one live cell per live row, never a whole column"
    );
}

#[test]
fn the_gutter_names_each_rows_live_scene() {
    let harness = harness([700.0, 480.0]);

    harness.get_by_label("Group 1: Background (live Aurora)");
    harness.get_by_label("Group 2: Foreground (live Basalt)");
    assert!(
        harness
            .query_by_label("Group 3: Effects (live Cinder)")
            .is_none(),
        "row 2 has no live Scene to name"
    );
    assert_eq!(
        harness.get_all_by_label_contains("(live ").count(),
        2,
        "only rows with a live Scene carry a name"
    );
}

#[test]
fn the_applied_scene_header_is_not_presented_as_all_rows_live() {
    let harness = harness([700.0, 480.0]);

    // The header says the Scene is applied, and it is not a live-cell label.
    harness.get_by_label("Aurora (applied Scene)");
    assert_eq!(
        harness.get_all_by_label_contains("LIVE Aurora").count(),
        1,
        "only row 0's Aurora cell is LIVE; the header is applied, not live"
    );
}

#[test]
fn performance_mode_marks_only_each_rows_live_cell_in_the_pure_decision() {
    let rows = live_rows(&[(0, "scene-a"), (1, "scene-b")]);

    assert_eq!(
        scene_cell_highlight(&rows, "scene-a", 0),
        SceneCellHighlight::Live
    );
    assert_eq!(
        scene_cell_highlight(&rows, "scene-b", 1),
        SceneCellHighlight::Live
    );
    assert_eq!(
        scene_cell_highlight(&rows, "scene-a", 1),
        SceneCellHighlight::None,
        "Scene A is not live in row 1 even though it is the applied Scene"
    );
    assert_eq!(
        scene_cell_highlight(&rows, "scene-c", 2),
        SceneCellHighlight::None,
        "a row with no live entry marks nothing"
    );
}

#[test]
fn performance_mode_ignores_the_applied_scene_when_rows_diverge() {
    let rows = live_rows(&[(0, "scene-a"), (1, "scene-b")]);

    for row in 0..3 {
        assert_eq!(
            scene_cell_highlight(&rows, "scene-c", row),
            SceneCellHighlight::None,
            "applied Scene C is not live at row {row}"
        );
    }
    assert_eq!(
        scene_cell_highlight(&rows, "scene-a", 0),
        SceneCellHighlight::Live
    );
    assert_eq!(
        scene_cell_highlight(&rows, "scene-b", 1),
        SceneCellHighlight::Live
    );
}

#[test]
fn performance_off_marks_only_each_rows_live_cell_in_the_pure_decision() {
    let rows = live_rows(&[(0, "scene-a"), (1, "scene-b")]);

    assert_eq!(
        scene_cell_highlight(&rows, "scene-a", 0),
        SceneCellHighlight::Live,
        "row 0 is live on Scene A even with performance mode off"
    );
    assert_eq!(
        scene_cell_highlight(&rows, "scene-b", 1),
        SceneCellHighlight::Live,
        "row 1 is live on Scene B even though Scene A is the applied Scene"
    );
    assert_eq!(
        scene_cell_highlight(&rows, "scene-a", 1),
        SceneCellHighlight::None,
        "the applied Scene must not light another row's live cell"
    );
    assert_eq!(
        scene_cell_highlight(&rows, "scene-a", 2),
        SceneCellHighlight::None,
        "the applied Scene must not light a row with no live Scene"
    );
}

/// The user-visible regression from BUGS.md: with performance mode off the
/// grid used to light the entire applied Scene column, hiding the fact that
/// other rows were live on different Scenes. Only the actual per-row live
/// cells may carry the LIVE marker.
#[test]
fn performance_off_marks_only_the_rows_live_scenes_in_the_grid() {
    let harness = harness_with_performance([700.0, 480.0], false);

    assert!(
        harness.query_by_label("LIVE Aurora Group 1").is_some(),
        "row 0's live Scene must be marked in the grid with performance off"
    );
    assert!(
        harness.query_by_label("LIVE Basalt Group 2").is_some(),
        "row 1's live Scene must be marked in the grid with performance off"
    );

    assert!(
        harness.query_by_label("LIVE Aurora Group 2").is_none(),
        "the applied Scene must not light a whole column while other rows are live"
    );
    assert!(
        harness.query_by_label("LIVE Aurora Group 3").is_none(),
        "the applied Scene must not light a row with no live Scene"
    );
    assert!(
        harness.query_by_label("LIVE Basalt Group 1").is_none(),
        "Basalt is live only in Group 1"
    );
    assert_eq!(
        harness.get_all_by_label_contains("LIVE ").count(),
        2,
        "exactly one live cell per live row, never a whole column"
    );
}

#[test]
fn scene_grid_live_rows_golden() {
    let mut harness = harness([700.0, 480.0]);
    harness.snapshot("scene_grid_live_rows");
}
