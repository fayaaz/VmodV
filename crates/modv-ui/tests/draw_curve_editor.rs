//! The Draw curve editor: a golden of the open window plus click-to-add,
//! point-drag and Shift+drag freehand gestures.
//!
//! Run `cargo test -p modv-ui --test draw_curve_editor`; the first run writes
//! `tests/snapshots/draw_curve_editor.png` and the next compares against it.

use egui::{Modifiers, PointerButton, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::widgets::DrawControl;
use modv_ui::widgets::draw_curve::{DRAW_POINTS, DrawCurve};
use modv_ui::widgets::tween_curve::{progress_to_x, value_to_y};
use modv_ui::{DrawCurveEditor, TweenView, UiState};

fn view() -> TweenView {
    let curve = DrawCurve::new(0.0, 1.0);
    TweenView {
        id: "input-gain".to_string(),
        data: curve.to_rows(),
        keyframe_times: Some(curve.times()),
        duration: 4000.0,
        direction: "normal".to_string(),
        easing: "linear".to_string(),
        looping: true,
        loop_mode: None,
        use_bpm: true,
        bpm_division: 200.0 / 3.0,
        duration_as_total_time: true,
        steps: 0.0,
        frames: 0,
        cycle_seconds: 2.0,
    }
}

fn editor_state() -> UiState {
    let mut state = UiState::new();
    state.status.bpm = 120.0;
    state.draw_editor = Some(DrawCurveEditor::new(
        "input-gain",
        "gain",
        DrawCurve::new(0.0, 1.0),
    ));
    state
}

fn harness(state: UiState) -> Harness<'static, UiState> {
    harness_with_view(state, view())
}

fn harness_with_view(state: UiState, base: TweenView) -> Harness<'static, UiState> {
    let harness = Harness::builder()
        .with_size([900.0, 700.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                // Emulate the engine snapshot catching up: after a gesture is
                // dispatched, the next view carries the edited curve.
                let mut current = base.clone();
                if let Some(editor) = &state.draw_editor {
                    current.data = editor.curve.to_rows();
                    current.keyframe_times = Some(editor.curve.times());
                }
                let mut control = DrawControl::new(
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

fn loop_mode_view(looping: bool, loop_mode: Option<String>) -> TweenView {
    TweenView {
        looping,
        loop_mode,
        ..view()
    }
}

fn canvas_plot(harness: &Harness<'static, UiState>) -> Rect {
    let outer = harness.get_by_label("Draw curve canvas").rect();
    Rect::from_min_max(outer.min + vec2(38.0, 8.0), outer.max - vec2(8.0, 20.0))
}

fn click_at(harness: &mut Harness<'static, UiState>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

#[test]
fn draw_curve_editor_snapshot() {
    let mut harness = harness(editor_state());
    harness.run();
    harness.run();
    let editor = harness.state().draw_editor.as_ref().expect("editor open");
    assert_eq!(editor.input_id, "input-gain");
    assert_eq!(editor.curve.rows(), 2);
    harness.get_by_label("Draw curve canvas");
    harness.snapshot("draw_curve_editor");
}

#[test]
fn clicking_adds_a_point_and_joining_the_descriptor() {
    let mut harness = harness(editor_state());
    harness.run();

    let plot = canvas_plot(&harness);
    let bounds = (0.0, 1.0);
    let point = pos2(progress_to_x(0.25, plot), value_to_y(0.75, bounds, plot));
    click_at(&mut harness, point);

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 2, "one descriptor update plus its link");
    assert_eq!(actions[0].action, "tweens/createTween");
    assert_eq!(
        actions[0].payload["data"],
        serde_json::json!([[0.5], [0.75], [0.5]]),
        "the clicked point joins the existing line"
    );
    assert_eq!(
        actions[0].payload["keyframeTimes"],
        serde_json::json!([0.0, 0.25, 1.0])
    );
    assert_eq!(actions[1].action, "inputs/createInputLink");
    assert_eq!(actions[1].payload["source"], "draw");

    harness.run();
    let editor = harness.state().draw_editor.as_ref().expect("editor open");
    assert_eq!(editor.curve.rows(), 3);
    assert!((editor.curve.sample(0.25) - 0.75).abs() < 0.01);
}

#[test]
fn dragging_a_handle_moves_only_that_point() {
    let mut harness = harness(editor_state());
    harness.run();

    let plot = canvas_plot(&harness);
    let bounds = (0.0, 1.0);
    let start = pos2(progress_to_x(0.0, plot), value_to_y(0.5, bounds, plot));
    let end = pos2(progress_to_x(0.3, plot), value_to_y(0.9, bounds, plot));

    harness.hover_at(start);
    harness.run();
    harness.drag_at(start);
    harness.run();
    harness.hover_at(end);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: end,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].action, "tweens/createTween");
    let data = actions[0].payload["data"].as_array().expect("rows");
    assert!(
        (data[0][0].as_f64().expect("value") - 0.9).abs() < 1e-5,
        "the first endpoint's time is pinned but its value moves: {data:?}"
    );
    assert_eq!(data[1], serde_json::json!([0.5]));
    assert!(
        actions[0].payload.get("keyframeTimes").is_none(),
        "two rows use the [from, to] shorthand"
    );
}

#[test]
fn shift_dragging_paints_the_whole_stroke() {
    let mut harness = harness(editor_state());
    harness.run();

    let plot = canvas_plot(&harness);
    let bounds = (0.0, 1.0);
    let start = pos2(progress_to_x(0.2, plot), value_to_y(0.8, bounds, plot));
    let end = pos2(progress_to_x(0.6, plot), value_to_y(0.2, bounds, plot));

    // Keep Shift held across the whole gesture; `event_modifiers` would reset
    // it after each queued event.
    harness.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    harness.run();
    harness.event(egui::Event::PointerMoved(start));
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: start,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::SHIFT,
    });
    harness.run();
    harness.event(egui::Event::PointerMoved(end));
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: end,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::SHIFT,
    });
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 2, "one descriptor update plus its link");
    assert_eq!(actions[0].action, "tweens/createTween");
    let payload = &actions[0].payload;
    assert_eq!(payload["direction"], "normal");
    assert_eq!(payload["easing"], "linear");
    assert_eq!(payload["durationAsTotalTime"], true);
    assert_eq!(payload["data"].as_array().map(Vec::len), Some(DRAW_POINTS));
    assert_eq!(
        payload["keyframeTimes"].as_array().map(Vec::len),
        Some(DRAW_POINTS)
    );

    let last = (DRAW_POINTS - 1) as f64;
    let at = |progress: f64| (progress * last).round() as usize;
    let value = |progress: f64| payload["data"][at(progress)][0].as_f64().expect("value");
    assert!(
        (value(0.2) - 0.8).abs() < 0.03,
        "the stroke start keeps its value: {}",
        value(0.2)
    );
    assert!(
        (value(0.6) - 0.2).abs() < 0.03,
        "the stroke end keeps its value: {}",
        value(0.6)
    );
    assert!(
        value(0.4) < 0.55 && value(0.4) > 0.45,
        "the middle of the stroke was filled in, not skipped: {}",
        value(0.4)
    );
    assert_eq!(actions[1].action, "inputs/createInputLink");
    assert_eq!(actions[1].payload["source"], "draw");

    let editor = harness.state().draw_editor.as_ref().expect("editor open");
    assert_eq!(editor.curve.rows(), DRAW_POINTS);
}

#[test]
fn ticking_pingpong_enables_loop_and_dispatches_pingpong() {
    let mut harness = harness_with_view(
        UiState::new(),
        loop_mode_view(false, Some("restart".to_string())),
    );
    harness.run();

    let pingpong = harness.query_by_label("ping-pong");
    assert!(
        pingpong.is_some(),
        "the draw editor exposes a ping-pong checkbox"
    );
    pingpong.expect("ping-pong checkbox").click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 2, "one descriptor update plus its link");
    assert_eq!(actions[0].action, "tweens/createTween");
    assert_eq!(actions[0].payload["loop"], true);
    assert_eq!(actions[0].payload["loopMode"], "pingpong");
}

#[test]
fn unticking_loop_clears_pingpong_and_dispatches_restart() {
    let mut harness = harness_with_view(
        UiState::new(),
        loop_mode_view(true, Some("pingpong".to_string())),
    );
    harness.run();
    assert!(
        harness.query_by_label("ping-pong").is_some(),
        "the ping-pong mode is represented by its own checkbox"
    );

    harness.get_by_label("loop").click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 2, "one descriptor update plus its link");
    assert_eq!(actions[0].action, "tweens/createTween");
    assert_eq!(actions[0].payload["loop"], false);
    assert_eq!(actions[0].payload["loopMode"], "restart");
}
