//! The tween curve editor: a golden of the open window and a simulated handle
//! drag that checks the descriptor queued for the engine.
//!
//! Run `cargo test -p modv-ui --test tween_curve_editor`; the first run writes
//! `tests/snapshots/tween_curve_editor.png` and the next compares against it.

use egui::{Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{
    TweenCurveEditor, TweenView, UiState,
    widgets::TweenControl,
    widgets::tween_curve::{TweenCurve, progress_to_x, value_to_y},
};

fn view() -> TweenView {
    TweenView {
        id: "input-gain".to_string(),
        data: vec![vec![0.0, 0.5], vec![0.5, 1.0], vec![1.0, 0.0]],
        keyframe_times: Some(vec![0.0, 0.35, 1.0]),
        duration: 1000.0,
        direction: "alternate".to_string(),
        easing: "easeInOutSine".to_string(),
        looping: true,
        use_bpm: true,
        bpm_division: 16.0,
        duration_as_total_time: false,
        steps: 0.0,
        frames: 32,
        // 16th-note division at 120 BPM, one leg of the 3-row shape.
        cycle_seconds: 0.48,
    }
}

fn editor_state() -> UiState {
    let mut state = UiState::new();
    state.status.bpm = 120.0;
    state.tween_editor = Some(TweenCurveEditor::new(
        "input-gain",
        "gain",
        TweenCurve::from_view(&view()).expect("curve"),
    ));
    state
}

fn harness(state: UiState) -> Harness<'static, UiState> {
    let view = view();
    let harness = Harness::builder()
        .with_size([900.0, 700.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                // Emulate the engine snapshot catching up: once the editor
                // dispatched a curve, the next view carries those keyframes.
                let mut current = view.clone();
                if let Some(editor) = &state.tween_editor {
                    current.data = editor.curve.data.clone();
                    current.keyframe_times = editor
                        .curve
                        .explicit_times
                        .then(|| editor.curve.times.clone());
                }
                let mut control = TweenControl::new(
                    "gain",
                    Some("input-gain".to_string()),
                    Some(current),
                    0.0,
                    1.0,
                );
                control.draw(ui, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness
}

#[test]
fn tween_curve_editor_snapshot() {
    let mut harness = harness(editor_state());
    harness.run();
    harness.run();
    let editor = harness.state().tween_editor.as_ref().expect("editor open");
    assert_eq!(editor.input_id, "input-gain");
    assert_eq!(editor.curve.rows(), 3);
    assert_eq!(editor.curve.channels(), 2);
    harness.get_by_label("Tween curve canvas");
    harness.snapshot("tween_curve_editor");
}

#[test]
fn dragging_a_handle_queues_the_full_descriptor_once() {
    let mut harness = harness(editor_state());
    harness.run();

    // The canvas reports its screen rect through accessibility, so the drag can
    // target the same plot maths the widget uses.
    let outer = harness.get_by_label("Tween curve canvas").rect();
    let plot = Rect::from_min_max(outer.min + vec2(38.0, 8.0), outer.max - vec2(8.0, 20.0));
    let curve = TweenCurve::from_view(&view()).expect("curve");
    let bounds = curve.value_bounds(0.0, 1.0);
    // Row 1, channel 0: (0.35, 0.5) -> (0.5, 0.9).
    let start = pos2(progress_to_x(0.35, plot), value_to_y(0.5, bounds, plot));
    let end = pos2(progress_to_x(0.5, plot), value_to_y(0.9, bounds, plot));

    harness.hover_at(start);
    harness.run();
    harness.drag_at(start);
    harness.run();
    harness.hover_at(end);
    harness.run();
    // A raw release, not `drop_at`: that also emits PointerGone, which clears
    // the pointer position before the release is handled.
    harness.event(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 2, "one descriptor update plus its link");
    assert_eq!(actions[0].action, "tweens/createTween");
    let payload = &actions[0].payload;
    assert_eq!(payload["data"][0], serde_json::json!([0.0, 0.5]));
    assert!(
        (payload["data"][1][0].as_f64().expect("value") - 0.9).abs() < 0.02,
        "channel 0 dragged to 0.9: {}",
        payload["data"][1][0]
    );
    assert_eq!(payload["data"][1][1], 1.0, "the other channel is unchanged");
    assert!(
        (payload["keyframeTimes"][1].as_f64().expect("time") - 0.5).abs() < 0.02,
        "the handle moved along x: {}",
        payload["keyframeTimes"][1]
    );
    assert_eq!(payload["data"].as_array().map(Vec::len), Some(3));
    assert_eq!(payload["durationAsTotalTime"], false);
    assert_eq!(actions[1].action, "inputs/createInputLink");

    // The editor's curve is mirrored so the window keeps the new position.
    let editor = harness.state().tween_editor.as_ref().expect("editor open");
    assert!(
        (editor.curve.value(1, 0) - 0.9).abs() < 0.02,
        "local curve mirrored the drag"
    );
}
