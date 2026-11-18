//! The tween palette editor: swatch strip, add/remove/colour edits and the
//! golden of the open curve window with a palette tween.
//!
//! Run `cargo test -p modv-ui --test tween_palette_editor`; the first run
//! writes `tests/snapshots/tween_palette_editor.png` and the next compares.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{
    TweenCurveEditor, TweenView, UiState, widgets::TweenControl, widgets::tween_curve::TweenCurve,
};

/// Ball/Waveform/Polygon `color`: byte-scale RGB rows.
fn palette_view() -> TweenView {
    TweenView {
        id: "input-color".to_string(),
        data: vec![
            vec![0.0, 0.0, 0.0],
            vec![255.0, 64.0, 0.0],
            vec![255.0, 200.0, 0.0],
            vec![0.0, 0.0, 255.0],
        ],
        keyframe_times: Some(vec![0.0, 0.3, 0.6, 1.0]),
        duration: 10_000.0,
        direction: "alternate".to_string(),
        easing: "linear".to_string(),
        looping: true,
        use_bpm: true,
        bpm_division: 16.0,
        duration_as_total_time: false,
        steps: 0.0,
        frames: 0,
        cycle_seconds: 1.0,
    }
}

fn build_harness(view: TweenView, open_editor: bool) -> Harness<'static, UiState> {
    let mut state = UiState::new();
    state.status.bpm = 120.0;
    if open_editor {
        state.tween_editor = Some(TweenCurveEditor::new(
            "input-color",
            "color",
            TweenCurve::from_view(&view).expect("palette curve"),
        ));
    }
    let harness = Harness::builder()
        .with_size([900.0, 700.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                let mut control = TweenControl::new(
                    "color",
                    Some("input-color".to_string()),
                    Some(view.clone()),
                    0.0,
                    255.0,
                );
                control.draw(ui, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    // egui's colour picker includes an emoji copy button; kittest's default
    // panic-on-missing-glyph would trip on the theme's IBM Plex fonts.
    harness
        .ctx
        .set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
    harness
}

fn opened() -> Harness<'static, UiState> {
    let mut harness = build_harness(palette_view(), true);
    harness.run();
    harness.run();
    harness
}

#[test]
fn the_collapsed_row_shows_the_palette_preview() {
    let mut harness = build_harness(palette_view(), false);
    harness.run();

    harness.get_by_label("Palette preview swatch 0 #000000");
    harness.get_by_label("Palette preview swatch 1 #FF4000");
    harness.get_by_label("Palette preview swatch 3 #0000FF");
    assert!(
        harness.query_by_label("Palette").is_none(),
        "the editable strip only lives in the curve window"
    );
}

#[test]
fn adding_a_swatch_dispatches_the_full_descriptor_once() {
    let mut harness = opened();
    harness.get_by_label("+").click();
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "palette edits never re-create the link");
    assert_eq!(actions[0].action, "tweens/createTween");
    let payload = &actions[0].payload;
    assert_eq!(payload["id"], "input-color");
    assert_eq!(
        payload["data"],
        serde_json::json!([
            [0.0, 0.0, 0.0],
            [255.0, 64.0, 0.0],
            [255.0, 200.0, 0.0],
            [0.0, 0.0, 255.0],
            [0.0, 0.0, 255.0]
        ]),
        "the new swatch duplicates the last colour"
    );
    assert_eq!(
        payload["keyframeTimes"],
        serde_json::json!([0.0, 0.25, 0.5, 0.75, 1.0]),
        "explicit times are re-normalised evenly"
    );
    assert_eq!(payload["duration"], 10_000.0);
    assert_eq!(payload["useBpm"], true);
    assert_eq!(payload["bpmDivision"], 16.0);
}

#[test]
fn right_click_removes_a_swatch_and_keeps_two_rows() {
    let mut harness = opened();
    harness
        .get_by_label("Palette swatch 1 #FF4000")
        .click_secondary();
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action, "tweens/createTween");
    assert_eq!(
        actions[0].payload["data"],
        serde_json::json!([[0.0, 0.0, 0.0], [255.0, 200.0, 0.0], [0.0, 0.0, 255.0]])
    );
    assert_eq!(
        actions[0].payload["keyframeTimes"],
        serde_json::json!([0.0, 0.5, 1.0])
    );

    // Two rows are the minimum: a second removal is a no-op.
    let mut two_row = palette_view();
    two_row.data.truncate(2);
    two_row.keyframe_times = None;
    let mut harness = build_harness(two_row, true);
    harness.run();
    harness.run();
    harness
        .get_by_label("Palette swatch 0 #000000")
        .click_secondary();
    harness.run();
    assert!(harness.state_mut().take_actions().is_empty());
}

#[test]
fn editing_the_hex_field_dispatches_the_swatch_colour() {
    let mut harness = opened();
    harness.get_by_label("Palette swatch 0 #000000").click();
    harness.run();

    let field = harness.get_by_label("Swatch 0 hex");
    field.click();
    harness.run();
    assert!(
        harness.get_by_label("Swatch 0 hex").is_focused(),
        "the hex field takes keyboard focus"
    );
    harness
        .get_by_label("Swatch 0 hex")
        .set_text_value_accesskit("#00FF00");
    harness.run();
    assert_eq!(
        harness.get_by_label("Swatch 0 hex").value().as_deref(),
        Some("#00FF00"),
        "the hex value can be replaced"
    );
    harness.key_press(egui::Key::Enter);
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert_eq!(actions.len(), 1, "one colour edit, one descriptor");
    assert_eq!(actions[0].action, "tweens/createTween");
    let payload = &actions[0].payload;
    assert_eq!(
        payload["data"],
        serde_json::json!([
            [0.0, 255.0, 0.0],
            [255.0, 64.0, 0.0],
            [255.0, 200.0, 0.0],
            [0.0, 0.0, 255.0]
        ]),
        "only the edited swatch changes"
    );
    assert_eq!(
        payload["keyframeTimes"],
        serde_json::json!([0.0, 0.3, 0.6, 1.0]),
        "a colour edit keeps the timing"
    );
}

#[test]
fn tween_palette_editor_snapshot() {
    let mut harness = opened();
    harness.get_by_label("Palette");
    harness.get_by_label("Tween curve canvas");
    harness.snapshot("tween_palette_editor");
}
