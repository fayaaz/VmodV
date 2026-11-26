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

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
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
