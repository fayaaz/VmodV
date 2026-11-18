//! Kittest coverage for the native Audio panel.
//!
//! The panel is the native counterpart of Electron's `AudioVideo.vue` input
//! picker: system audio (PipeWire monitor), the host default input and every
//! enumerated cpal device, with the active choice marked and a live switch
//! queued as a [`UiCommand::SetAudioSource`].

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::state::SidePanelTab;
use modv_ui::{
    AudioDeviceView, AudioSelectionView, AudioView, EngineHealth, EngineSnapshot, UiCommand,
    UiState,
};

fn demo_snapshot() -> EngineSnapshot {
    EngineSnapshot {
        audio_devices: vec![
            AudioDeviceView {
                id: "alsa:usb".to_string(),
                name: "USB Microphone".to_string(),
                channels: 2,
                capture_sample_rate: 48_000,
            },
            AudioDeviceView {
                id: "alsa:builtin".to_string(),
                name: "Built-in Audio".to_string(),
                channels: 4,
                capture_sample_rate: 48_000,
            },
        ],
        audio: AudioView {
            bpm: 128.0,
            sample_rate: Some(48_000),
            ..Default::default()
        },
        audio_selection: AudioSelectionView {
            source: "pipewire".to_string(),
            device: None,
            capture_sample_rate: Some(48_000),
            error: None,
        },
        health: EngineHealth {
            audio_active: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn audio_panel_lists_the_system_monitor_default_input_and_devices() {
    let snapshot = demo_snapshot();
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Audio;
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    // The banner names the active source, its state and the live capture rate.
    harness.get_by_label("IN USE");
    harness.get_by_label("System audio");
    harness.get_by_label("Captures what your speakers play");
    harness.get_by_label("PipeWire monitor · 48 kHz · pipewire");

    // The two source groups, with the default input sorted first.
    harness.get_by_label("OUTPUT MONITOR");
    harness.get_by_label("INPUT DEVICES");
    harness.get_by_label("2 inputs");
    harness.get_by_label("System default input");
    harness.get_by_label("USB Microphone");
    harness.get_by_label("Built-in Audio");
    // Picker rows show the rate capture negotiates (48 kHz), never cpal's
    // 44.1 kHz `default_input_config()` heuristic.
    assert_eq!(harness.get_all_by_label("2 ch · 48 kHz").count(), 1);
    harness.get_by_label("4 ch · 48 kHz");
    harness.get_by_label("audio: engine · 48 kHz");
    // The tempo readouts stay compact; the capture rate is shown by the
    // audio indicator above, not repeated next to the BPM.
    assert_eq!(harness.get_all_by_label("BPM 128").count(), 2);
}

#[test]
fn the_active_banner_resolves_the_selected_device() {
    let mut snapshot = demo_snapshot();
    snapshot.audio_selection = AudioSelectionView {
        source: "input".to_string(),
        device: Some("alsa:usb".to_string()),
        capture_sample_rate: Some(48_000),
        error: None,
    };
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Audio;
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    harness.get_by_label("2 ch · 48 kHz · alsa:usb");
    harness.get_by_label("Microphone or audio interface");
}

#[test]
fn clicking_a_device_queues_the_live_switch() {
    let snapshot = demo_snapshot();
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Audio;
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    harness.get_by_label("USB Microphone").click();
    harness.run();

    assert!(
        harness
            .state()
            .commands
            .contains(&UiCommand::SetAudioSource {
                source: "input".to_string(),
                device: Some("alsa:usb".to_string()),
            }),
        "clicking a device must queue the switch, got {:?}",
        harness.state().commands
    );
}

#[test]
fn the_refresh_button_requests_a_new_device_scan() {
    let snapshot = demo_snapshot();
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Audio;
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    harness.get_by_label("Refresh devices").click();
    harness.run();

    assert!(
        harness
            .state()
            .commands
            .contains(&UiCommand::RefreshAudioDevices),
        "the Refresh button must request a rescan, got {:?}",
        harness.state().commands
    );
}

#[test]
fn a_failed_live_switch_is_shown_in_the_panel() {
    let mut snapshot = demo_snapshot();
    snapshot.audio_selection.error = Some("no input device matches \"missing\"".to_string());
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Audio;
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();

    harness.get_by_label("no input device matches \"missing\"");
}
