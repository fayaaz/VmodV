//! The tempo readouts: status bar/menu bar, the Tempo window and the Info
//! panel's Tempo row.
//!
//! The rate shown next to the BPM is the live capture feed's negotiated rate
//! (`AudioFeed::sample_rate()`), never cpal's 44.1 kHz
//! `default_input_config()` heuristic. A silent engine omits the rate and the
//! Tempo window says so explicitly.
//!
//! Run `cargo test -p modv-ui --test bpm_window`; the first run writes
//! `tests/snapshots/bpm_window.png` and the next compares against it.

use accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use modv_ui::state::SidePanelTab;
use modv_ui::{AudioSelectionView, AudioView, EngineHealth, EngineSnapshot, UiState};

fn snapshot(bpm: f64, capture_rate: Option<u32>) -> EngineSnapshot {
    EngineSnapshot {
        audio: AudioView {
            bpm,
            sample_rate: capture_rate,
            ..Default::default()
        },
        audio_selection: AudioSelectionView {
            source: "system".to_string(),
            device: None,
            capture_sample_rate: capture_rate,
            error: None,
        },
        health: EngineHealth {
            audio_active: capture_rate.is_some(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn harness(snapshot: EngineSnapshot, state: UiState) -> Harness<'static, UiState> {
    let harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness
}

#[test]
fn the_tempo_window_reports_the_capture_rate_without_duplicating_it_by_the_bpm() {
    // 128.4 BPM: the status bar rounds, the Tempo window keeps one decimal.
    let mut harness = harness(snapshot(128.4, Some(48_000)), UiState::new());
    harness.state_mut().show_bpm = true;
    harness.run();

    assert_eq!(
        harness.get_all_by_label("BPM 128").count(),
        2,
        "the menu bar and the status bar show the bare tempo"
    );
    harness.get_by_label("Detected 128.4 BPM");
    harness.get_by_label("Capture 48 kHz");
    harness.get_by_label("audio: engine · 48 kHz");
}

#[test]
fn a_silent_engine_omits_the_rate_and_the_tempo_window_says_so() {
    let mut harness = harness(snapshot(96.0, None), UiState::new());
    harness.state_mut().show_bpm = true;
    harness.run();

    assert_eq!(harness.get_all_by_label("BPM 96").count(), 2);
    harness.get_by_label("Detected 96.0 BPM");
    harness.get_by_label("Capture rate unavailable");
    harness.get_by_label("audio: silent");
    assert!(
        harness.query_by_label("Capture 48 kHz").is_none(),
        "a silent engine must not advertise a capture rate"
    );
}

#[test]
fn the_info_panel_tempo_row_agrees_with_the_status_bar() {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Info;
    let mut harness = harness(snapshot(128.0, Some(48_000)), state);
    harness.run();

    harness.get_by_label("Tempo");
    assert!(
        harness.get_all_by_label("BPM 128").count() >= 3,
        "the Info panel Tempo row plus the menu bar and status bar show the bare tempo"
    );
    harness.get_by_label("System audio (loopback)  (engine · 48 kHz)");
}

#[test]
fn bpm_window_snapshot() {
    let mut state = UiState::new();
    state.show_bpm = true;
    let mut harness = harness(snapshot(128.0, Some(48_000)), state);
    harness.run();
    harness.snapshot("bpm_window");
}

#[test]
fn space_bar_taps_tempo_and_dispatches_the_tap_actions() {
    let mut harness = harness(snapshot(0.0, Some(48_000)), UiState::new());
    harness.state_mut().show_bpm = true;
    harness
        .state_mut()
        .tap_times
        .push(web_time::Instant::now() - std::time::Duration::from_millis(500));
    harness.run();

    harness.key_press(egui::Key::Space);
    harness.run();

    let actions = harness.state_mut().take_actions();
    let source_index = actions
        .iter()
        .position(|action| {
            action.action == "beats/SET_BPM_SOURCE" && action.payload["source"] == "tap"
        })
        .unwrap_or_else(|| panic!("Space must select the tap source: {actions:?}"));
    let bpm_index = actions
        .iter()
        .position(|action| action.action == "beats/setBpm")
        .unwrap_or_else(|| panic!("Space must dispatch the tapped tempo: {actions:?}"));
    assert!(
        source_index < bpm_index,
        "the tap source must be selected before its BPM is set: {actions:?}"
    );
    assert_eq!(actions[bpm_index].payload["source"], "tap");
    let bpm = actions[bpm_index].payload["bpm"]
        .as_f64()
        .expect("the tapped BPM is numeric");
    assert!(
        bpm.is_finite() && (90.0..=150.0).contains(&bpm),
        "a 500 ms tap interval should be near 120 BPM, got {bpm}"
    );
}

#[test]
fn space_bar_taps_tempo_even_when_a_button_has_focus() {
    let mut harness = harness(snapshot(0.0, Some(48_000)), UiState::new());
    harness.state_mut().show_bpm = true;
    harness
        .state_mut()
        .tap_times
        .push(web_time::Instant::now() - std::time::Duration::from_millis(500));
    harness.run();

    // Tap itself activates on Space, which would mask the global-shortcut bug.
    // Reset taps is also a non-text button, but its native activation only
    // clears the seeded taps, so the broken shortcut leaves no actions.
    harness.get_by_label("Reset taps").focus();
    harness.run();
    assert!(
        harness.get_by_label("Reset taps").is_focused(),
        "the Reset taps button proves that a non-text widget has focus"
    );

    harness.key_press(egui::Key::Space);
    harness.run();

    let actions = harness.state_mut().take_actions();
    let source_index = actions
        .iter()
        .position(|action| {
            action.action == "beats/SET_BPM_SOURCE" && action.payload["source"] == "tap"
        })
        .unwrap_or_else(|| panic!("Space must select the tap source: {actions:?}"));
    let bpm_index = actions
        .iter()
        .position(|action| action.action == "beats/setBpm")
        .unwrap_or_else(|| panic!("Space must dispatch the tapped tempo: {actions:?}"));
    assert!(
        source_index < bpm_index,
        "the tap source must be selected before its BPM is set: {actions:?}"
    );
    assert_eq!(actions[bpm_index].payload["source"], "tap");
    let bpm = actions[bpm_index].payload["bpm"]
        .as_f64()
        .expect("the tapped BPM is numeric");
    assert!(
        bpm.is_finite() && (90.0..=150.0).contains(&bpm),
        "a 500 ms tap interval should be near 120 BPM, got {bpm}"
    );
}

#[test]
fn typing_a_bpm_value_switches_to_manual_and_dispatches_it() {
    let mut state = UiState::new();
    state.show_bpm = true;
    let mut harness = harness(snapshot(0.0, Some(48_000)), state);
    harness.run();

    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, "Set BPM")
            .is_some(),
        "the Tempo window must expose a single-line Set BPM text field"
    );
    {
        let field = harness.get_by_role_and_label(Role::TextInput, "Set BPM");
        field.focus();
    }
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Set BPM")
        .set_text_value_accesskit("128");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();

    let actions = harness.state_mut().take_actions();
    let source_index = actions
        .iter()
        .position(|action| {
            action.action == "beats/SET_BPM_SOURCE" && action.payload["source"] == "manual"
        })
        .unwrap_or_else(|| panic!("typing a BPM must select the manual source: {actions:?}"));
    let bpm_index = actions
        .iter()
        .position(|action| action.action == "beats/setBpm")
        .unwrap_or_else(|| panic!("Enter must dispatch the typed tempo: {actions:?}"));
    assert!(
        source_index < bpm_index,
        "the manual source must be selected before its BPM is set: {actions:?}"
    );
    assert_eq!(actions[bpm_index].payload["bpm"], 128.0);
    assert_eq!(actions[bpm_index].payload["source"], "manual");
}

#[test]
fn a_second_enter_does_not_redispatch_the_typed_bpm() {
    let mut state = UiState::new();
    state.show_bpm = true;
    let mut harness = harness(snapshot(0.0, Some(48_000)), state);
    harness.run();

    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, "Set BPM")
            .is_some(),
        "the Tempo window must expose a single-line Set BPM text field"
    );
    {
        let field = harness.get_by_role_and_label(Role::TextInput, "Set BPM");
        field.focus();
    }
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Set BPM")
        .set_text_value_accesskit("128");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();

    let first_actions = harness.state_mut().take_actions();
    let committed = first_actions
        .iter()
        .find(|action| action.action == "beats/setBpm" && action.payload["bpm"] == 128.0)
        .unwrap_or_else(|| panic!("Enter must commit the typed 128 BPM first: {first_actions:?}"));
    assert_eq!(committed.payload["source"], "manual");

    harness
        .get_by_role_and_label(Role::TextInput, "Set BPM")
        .focus();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();

    let second_actions = harness.state_mut().take_actions();
    assert!(
        second_actions.iter().all(|action| {
            action.action != "beats/setBpm" && action.action != "beats/SET_BPM_SOURCE"
        }),
        "a second Enter must not re-dispatch the committed BPM draft: {second_actions:?}"
    );
}

#[test]
fn invalid_typed_bpm_is_ignored() {
    let mut state = UiState::new();
    state.show_bpm = true;
    let mut harness = harness(snapshot(0.0, Some(48_000)), state);
    harness.run();

    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, "Set BPM")
            .is_some(),
        "the Tempo window must expose a single-line Set BPM text field"
    );
    {
        let field = harness.get_by_role_and_label(Role::TextInput, "Set BPM");
        field.focus();
    }
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "Set BPM")
        .set_text_value_accesskit("500");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();

    let actions = harness.state_mut().take_actions();
    assert!(
        actions.iter().all(|action| action.action != "beats/setBpm"),
        "an out-of-range BPM must not dispatch beats/setBpm: {actions:?}"
    );
}

#[test]
fn the_tempo_window_exposes_the_detector_bpm_range() {
    let mut state = UiState::new();
    state.show_bpm = true;
    state.bpm_source = "beatdetektor".to_string();
    let mut harness = harness(snapshot(120.0, Some(48_000)), state);
    harness.run();

    assert!(
        harness
            .query_by_role_and_label(Role::Slider, "Min BPM")
            .is_some(),
        "the Tempo window must expose a Min BPM slider for beatdetektor"
    );
    assert!(
        harness
            .query_by_role_and_label(Role::Slider, "Max BPM")
            .is_some(),
        "the Tempo window must expose a Max BPM slider for beatdetektor"
    );
    let min = harness.get_by_role_and_label(Role::Slider, "Min BPM");
    let max = harness.get_by_role_and_label(Role::Slider, "Max BPM");
    for slider in [&min, &max] {
        assert_eq!(slider.accesskit_node().min_numeric_value(), Some(30.0));
        assert_eq!(slider.accesskit_node().max_numeric_value(), Some(300.0));
    }

    harness
        .get_by_role_and_label(Role::Slider, "Min BPM")
        .set_value_accesskit(90.0);
    harness.run();
    harness
        .get_by_role_and_label(Role::Slider, "Max BPM")
        .set_value_accesskit(180.0);
    harness.run();

    let actions = harness.state_mut().take_actions();
    let range = actions
        .iter()
        .rev()
        .find(|action| action.action == "beats/SET_BPM_RANGE")
        .unwrap_or_else(|| {
            panic!("editing the range must dispatch beats/SET_BPM_RANGE: {actions:?}")
        });
    assert_eq!(range.payload["min"], 90.0);
    assert_eq!(range.payload["max"], 180.0);
}

#[test]
fn an_invalid_bpm_range_does_not_dispatch() {
    let mut state = UiState::new();
    state.show_bpm = true;
    state.bpm_source = "beatdetektor".to_string();
    let mut harness = harness(snapshot(120.0, Some(48_000)), state);
    harness.run();

    // Query both controls first so missing range controls fail this test too.
    assert!(
        harness
            .query_by_role_and_label(Role::Slider, "Min BPM")
            .is_some(),
        "the Tempo window must expose a Min BPM slider for beatdetektor"
    );
    assert!(
        harness
            .query_by_role_and_label(Role::Slider, "Max BPM")
            .is_some(),
        "the Tempo window must expose a Max BPM slider for beatdetektor"
    );

    harness
        .get_by_role_and_label(Role::Slider, "Min BPM")
        .set_value_accesskit(250.0);
    harness.run();
    harness
        .get_by_role_and_label(Role::Slider, "Max BPM")
        .set_value_accesskit(100.0);
    harness.run();

    let actions = harness.state_mut().take_actions();
    for action in actions
        .iter()
        .filter(|action| action.action == "beats/SET_BPM_RANGE")
    {
        let min = action.payload["min"]
            .as_f64()
            .expect("a dispatched range has a numeric minimum");
        let max = action.payload["max"]
            .as_f64()
            .expect("a dispatched range has a numeric maximum");
        assert!(
            min.is_finite() && max.is_finite() && min < max,
            "a crossed or non-finite range must not dispatch: {action:?}"
        );
        assert!(
            (30.0..=300.0).contains(&min) && (30.0..=300.0).contains(&max),
            "a dispatched range must stay within 30..=300 BPM: {action:?}"
        );
    }
}
