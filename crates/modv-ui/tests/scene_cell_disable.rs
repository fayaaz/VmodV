//! The Scenes grid's per-cell performance toggle.
//!
//! The small dot in a cell's top-left corner disables that cell in performance
//! mode without applying the Scene. In real data the toggle never reached
//! `performance.disabledCells`, so these tests press the dot itself and assert
//! the exact `scenes/setCellEnabled` payload with no `scenes/applyScene`.
//!
//! The disabled decoration is asserted through the pure draw-state helper and
//! the `scene_grid_row_monitor` golden (row 1 is disabled).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::panels::scene_grid::cell_enable_style;
use modv_ui::{EngineSnapshot, PerformanceView, SceneCellView, SceneView, UiAction, UiState};

const CELL_WIDTH: f32 = 154.0;
const CELL_HEIGHT: f32 = 76.0;

fn snapshot(disabled: bool) -> EngineSnapshot {
    EngineSnapshot {
        scenes: vec![SceneView {
            id: "scene-1".to_string(),
            name: "Lone Chrome".to_string(),
            enabled: true,
            timing: None,
            cells: vec![SceneCellView {
                row: 0,
                module_names: vec!["Bands".to_string()],
                disabled,
            }],
        }],
        scene_rows: vec!["Background".to_string()],
        performance: PerformanceView {
            enabled: true,
            current_scene_id: Some("scene-1".to_string()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn harness(disabled: Arc<AtomicBool>) -> Harness<'static, UiState> {
    let state = UiState::new();
    let harness = Harness::builder()
        .with_size([560.0, 440.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let snapshot = snapshot(disabled.load(Ordering::Relaxed));
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
    harness
}

/// The first cell's rect: the column header is an accessible button, and the
/// grid places the first cell just below it.
fn first_cell(harness: &Harness<'_, UiState>) -> egui::Rect {
    let header = harness.get_by_label("Lone Chrome").rect();
    egui::Rect::from_min_size(
        egui::pos2(header.left(), header.bottom() + 3.0),
        egui::vec2(CELL_WIDTH, CELL_HEIGHT),
    )
}

/// The dot's hit centre, matching `draw_scene_cell`'s placement.
fn dot_center(cell: egui::Rect) -> egui::Pos2 {
    cell.left_top() + egui::vec2(10.0, 27.0)
}

fn primary_click(harness: &mut Harness<'static, UiState>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.run();
    }
}

fn secondary_click(harness: &mut Harness<'static, UiState>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.run();
    }
}

/// The performance bar also emits its own config actions while it settles;
/// the cell tests only care about the toggle and the Scene apply.
fn relevant(actions: &[UiAction]) -> Vec<&UiAction> {
    actions
        .iter()
        .filter(|action| {
            action.action == "scenes/setCellEnabled" || action.action == "scenes/applyScene"
        })
        .collect()
}

#[test]
fn clicking_the_dot_toggles_the_cell_without_applying_the_scene() {
    let disabled = Arc::new(AtomicBool::new(false));
    let mut harness = harness(disabled.clone());
    harness.run();

    let cell = first_cell(&harness);
    primary_click(&mut harness, dot_center(cell));

    let actions = harness.state_mut().take_actions();
    let actions = relevant(&actions);
    assert_eq!(actions.len(), 1, "one toggle action: {actions:?}");
    assert_eq!(actions[0].action, "scenes/setCellEnabled");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({"sceneId": "scene-1", "rowIndex": 0, "enabled": false}),
        "clicking the enabled dot disables the cell in performance mode"
    );

    // The engine applies the toggle; the next click must offer to re-enable.
    disabled.store(true, Ordering::Relaxed);
    harness.run();
    let cell = first_cell(&harness);
    primary_click(&mut harness, dot_center(cell));

    let actions = harness.state_mut().take_actions();
    let actions = relevant(&actions);
    assert_eq!(
        actions
            .iter()
            .filter(|action| action.action == "scenes/applyScene")
            .count(),
        0,
        "the cell must not apply the Scene when the dot is pressed: {actions:?}"
    );
    assert_eq!(actions.len(), 1, "one toggle action: {actions:?}");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({"sceneId": "scene-1", "rowIndex": 0, "enabled": true}),
        "clicking the disabled dot re-enables the cell"
    );
}

#[test]
fn the_dot_is_a_real_accessible_button() {
    let mut harness = harness(Arc::new(AtomicBool::new(false)));
    harness.run();

    // The tooltip doubles as the accessible name; the dot is its own widget,
    // not a hit rectangle shared with the cell.
    let dot = harness.get_by_label("Disable this cell in performance mode");
    dot.click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    let actions = relevant(&actions);
    assert_eq!(actions.len(), 1, "one toggle action: {actions:?}");
    assert_eq!(actions[0].action, "scenes/setCellEnabled");
}

#[test]
fn the_context_menu_toggle_emits_the_same_payload() {
    let disabled = Arc::new(AtomicBool::new(false));
    let mut harness = harness(disabled.clone());
    harness.run();

    let cell = first_cell(&harness);
    secondary_click(&mut harness, cell.center());
    harness
        .get_by_label("Disable cell in performance mode")
        .click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    let actions = relevant(&actions);
    assert_eq!(actions.len(), 1, "one toggle action: {actions:?}");
    assert_eq!(actions[0].action, "scenes/setCellEnabled");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({"sceneId": "scene-1", "rowIndex": 0, "enabled": false})
    );

    disabled.store(true, Ordering::Relaxed);
    harness.run();
    let cell = first_cell(&harness);
    secondary_click(&mut harness, cell.center());
    harness
        .get_by_label("Enable cell in performance mode")
        .click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    let actions = relevant(&actions);
    assert_eq!(actions.len(), 1, "one toggle action: {actions:?}");
    assert_eq!(
        actions[0].payload,
        serde_json::json!({"sceneId": "scene-1", "rowIndex": 0, "enabled": true})
    );
}

#[test]
fn the_disabled_decoration_is_red_and_labelled_off() {
    let enabled = cell_enable_style(false);
    assert_eq!(enabled.marker, modv_ui::theme::OK);
    assert!(enabled.border.is_none(), "enabled cells keep their border");
    assert!(enabled.scrim.is_none());
    assert_eq!(enabled.chip, None);

    let disabled = cell_enable_style(true);
    assert_eq!(disabled.marker, modv_ui::theme::DANGER);
    assert_eq!(
        disabled.border,
        Some(modv_ui::theme::DANGER),
        "a disabled cell gets a red border"
    );
    assert!(
        disabled.scrim.is_some(),
        "the hover preview is dimmed under a scrim"
    );
    assert_eq!(disabled.chip, Some("OFF"));
}
