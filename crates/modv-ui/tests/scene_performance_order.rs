//! Kittest coverage for the Scenes performance bar's Scene-order control.
//!
//! Electron's `Scenes.vue` exposes `performance.order` as a "Scene order"
//! select (In order / Random). The VmodV bar shows the same control and
//! dispatches the same `scenes/updatePerformance` payload.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, PerformanceView, SceneCellView, SceneView, UiState};

fn snapshot(order: &str, enabled: bool) -> EngineSnapshot {
    EngineSnapshot {
        scenes: vec![SceneView {
            id: "scene-a".to_string(),
            name: "Lone Chrome".to_string(),
            enabled: true,
            timing: Some((4.0, 8.0)),
            cells: vec![SceneCellView {
                row: 0,
                module_names: vec!["Bands".to_string()],
                disabled: false,
            }],
        }],
        scene_rows: vec!["Foreground".to_string()],
        performance: PerformanceView {
            enabled,
            order: order.to_string(),
            min_value: 8.0,
            max_value: 24.0,
            transition_seconds: 2.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn build(order: &str, enabled: bool) -> Harness<'static, UiState> {
    let snapshot = snapshot(order, enabled);
    let state = UiState::new();
    Harness::builder()
        .with_size([560.0, 320.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                ui.horizontal_wrapped(|ui| {
                    modv_ui::panels::draw_performance_bar(ui, &snapshot, &mut state.actions);
                });
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
        )
}

#[test]
fn the_performance_bar_labels_the_saved_scene_order() {
    let mut in_order = build("in-order", false);
    in_order.run();
    // The static label is painted text (an accesskit value on a Label node).
    assert!(
        in_order.get_all_by_value("Scene order").count() >= 1,
        "the control is labelled Scene order"
    );
    // The ComboBox exposes the selected option as its value.
    in_order.get_by_value("In order");

    let mut random = build("random", false);
    random.run();
    random.get_by_value("Random");
}

#[test]
fn choosing_random_while_performance_runs_dispatches_the_order() {
    // Performance is running: the engine reads `order` on every pick, so the
    // control stays enabled and a live change applies to the next Scene.
    let mut harness = build("in-order", true);
    harness.run();

    harness.get_by_value("In order").click();
    harness.run();
    harness.get_by_label("Random").click();
    harness.run();

    let action = harness
        .state()
        .actions
        .iter()
        .find(|action| {
            action.action == "scenes/updatePerformance" && action.payload.get("order").is_some()
        })
        .unwrap_or_else(|| {
            panic!(
                "choosing Random must dispatch the order, got {:?}",
                harness.state().actions
            )
        });
    assert_eq!(action.payload, serde_json::json!({ "order": "random" }));
}

#[test]
fn an_untouched_bar_queues_no_timing_updates() {
    // A fresh `PerformanceView` stores zeros; the bar must clamp those for
    // display without `DragValue` reporting a change on every frame.
    let snapshot = EngineSnapshot::default();
    let state = UiState::new();
    let mut harness = Harness::builder()
        .with_size([820.0, 320.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                ui.horizontal_wrapped(|ui| {
                    modv_ui::panels::draw_performance_bar(ui, &snapshot, &mut state.actions);
                });
            },
            state,
        );
    harness.run();
    harness.run();
    harness.run();
    assert!(
        harness.state().actions.is_empty(),
        "an untouched bar must not queue updates: {:?}",
        harness.state().actions
    );
}
