//! Kittest coverage for bar-based Scene timing.
//!
//! The performance bar selects the project's timing unit and edits the global
//! min/max in that unit; the Scene timing dialog does the same for an override.
//! The values in the snapshot are already in the active unit (the engine
//! converts to seconds only at use), so the controls must never relabel them.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{
    AudioView, EngineSnapshot, PerformanceView, SceneCellView, SceneView, TimingUnit, UiState,
};

fn scene(timing: Option<(f64, f64)>) -> SceneView {
    SceneView {
        id: "scene-a".to_string(),
        name: "Lone Chrome".to_string(),
        enabled: true,
        timing,
        cells: vec![SceneCellView {
            row: 0,
            module_names: vec!["Bands".to_string()],
            disabled: false,
        }],
    }
}

fn snapshot(unit: TimingUnit, timing: Option<(f64, f64)>) -> EngineSnapshot {
    EngineSnapshot {
        scenes: vec![scene(timing)],
        scene_rows: vec!["Foreground".to_string()],
        performance: PerformanceView {
            enabled: false,
            timing_unit: unit,
            min_value: if unit.is_bars() { 4.0 } else { 8.0 },
            max_value: if unit.is_bars() { 12.0 } else { 24.0 },
            transition_seconds: 2.0,
            ..Default::default()
        },
        audio: AudioView {
            bpm: 120.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn build(
    snapshot: EngineSnapshot,
    timing_edit: Option<(SceneView, PerformanceView)>,
) -> Harness<'static, UiState> {
    let mut state = UiState::new();
    if let Some((scene, performance)) = timing_edit {
        state.scene_grid.begin_timing(&scene, &performance);
    }
    Harness::builder()
        .with_size([720.0, 420.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
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
fn choosing_bars_dispatches_the_timing_unit_conversion() {
    let mut harness = build(snapshot(TimingUnit::Seconds, None), None);
    harness.run();

    harness.get_by_value("Seconds").click();
    harness.run();
    harness.get_by_label("Bars").click();
    harness.run();

    let action = harness
        .state()
        .actions
        .iter()
        .find(|action| action.action == "scenes/setTimingUnit")
        .unwrap_or_else(|| {
            panic!(
                "choosing Bars must dispatch scenes/setTimingUnit, got {:?}",
                harness.state().actions
            )
        });
    assert_eq!(action.payload, serde_json::json!({ "unit": "bars" }));
}

#[test]
fn bar_inputs_show_the_active_unit_and_the_resolved_duration() {
    let mut harness = build(snapshot(TimingUnit::Bars, None), None);
    harness.run();

    harness.get_by_value("Bars");
    let hint = "4.00–12.00 bars ≈ 8.0–24.0 s at 120 BPM";
    assert!(
        harness.get_all_by_value(hint).count() >= 1,
        "the bar hint resolves the live tempo: expected {hint:?}"
    );
}

#[test]
fn scene_timing_dialog_round_trips_the_override_min_and_max() {
    let unit = TimingUnit::Bars;
    let scene_view = scene(Some((1.5, 3.5)));
    let performance = snapshot(unit, None).performance;
    let mut harness = build(
        snapshot(unit, Some((1.5, 3.5))),
        Some((scene_view, performance)),
    );
    harness.run();

    let hint = "1.50–3.50 bars ≈ 3.0–7.0 s at 120 BPM";
    assert!(
        harness.get_all_by_value(hint).count() >= 1,
        "the dialog shows the override's own minimum and maximum"
    );

    harness.get_by_label("Save override").click();
    harness.run();

    let action = harness
        .state()
        .actions
        .iter()
        .find(|action| action.action == "scenes/updateSceneTiming")
        .unwrap_or_else(|| {
            panic!(
                "Save override must dispatch scenes/updateSceneTiming, got {:?}",
                harness.state().actions
            )
        });
    assert_eq!(
        action.payload,
        serde_json::json!({
            "sceneId": "scene-a",
            "minSeconds": 1.5,
            "maxSeconds": 3.5,
        }),
        "the dialog must not replace the minimum with the maximum"
    );
}

#[test]
fn scene_timing_dialog_can_reset_the_override() {
    let unit = TimingUnit::Seconds;
    let scene_view = scene(Some((1.0, 3.0)));
    let performance = snapshot(unit, None).performance;
    let mut harness = build(
        snapshot(unit, Some((1.0, 3.0))),
        Some((scene_view, performance)),
    );
    harness.run();

    harness.get_by_label("Reset override").click();
    harness.run();

    let action = harness
        .state()
        .actions
        .iter()
        .find(|action| action.action == "scenes/resetSceneTiming")
        .unwrap_or_else(|| {
            panic!(
                "Reset override must dispatch scenes/resetSceneTiming, got {:?}",
                harness.state().actions
            )
        });
    assert_eq!(action.payload, serde_json::json!({ "sceneId": "scene-a" }));
}
