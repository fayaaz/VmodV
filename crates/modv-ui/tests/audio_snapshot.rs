//! Golden snapshot of the native Audio panel.
//!
//! Unlike the default `shell` snapshot (Inspector tab), this opens the Audio
//! tab so the "IN USE" banner, the output-monitor/input-device split and the
//! filtered device rows are reviewed visually. The device list mirrors what
//! the filter leaves on a typical PipeWire host, not the raw 36-entry ALSA
//! probe list.
//!
//! Run `cargo test -p modv-ui --test audio_snapshot`; the first run writes
//! `tests/snapshots/audio_panel.png` and the next compares against it.

use egui_kittest::Harness;
use modv_ui::state::SidePanelTab;
use modv_ui::{AudioDeviceView, AudioSelectionView, EngineHealth, EngineSnapshot, UiState};

fn audio_snapshot() -> EngineSnapshot {
    EngineSnapshot {
        audio_devices: vec![
            AudioDeviceView {
                id: "alsa:pipewire".to_string(),
                name: "PipeWire Sound Server".to_string(),
                channels: 2,
                capture_sample_rate: 48_000,
            },
            AudioDeviceView {
                id: "alsa:hw:CARD=CODEC,DEV=0".to_string(),
                name: "USB Audio CODEC, USB Audio".to_string(),
                channels: 2,
                capture_sample_rate: 48_000,
            },
            AudioDeviceView {
                id: "alsa:hw:CARD=Generic_1,DEV=0".to_string(),
                name: "HD-Audio Generic, ALC1220 Analog".to_string(),
                channels: 2,
                capture_sample_rate: 48_000,
            },
        ],
        audio_selection: AudioSelectionView {
            source: "pipewire".to_string(),
            device: None,
            capture_sample_rate: None,
            error: None,
        },
        health: EngineHealth {
            adapter: "RADV GFX1201".to_string(),
            backend: "Vulkan".to_string(),
            frames_presented: 120,
            output_open: true,
            audio_active: true,
            ..Default::default()
        },
        fps: 60.0,
        frame_ms: 16.6,
        resolution: [960, 540],
        scene_rows: vec!["Background".to_string(), "Effects".to_string()],
        ..Default::default()
    }
}

#[test]
fn audio_panel_snapshot() {
    let snapshot = audio_snapshot();
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Audio;
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    harness.run();
    harness.snapshot("audio_panel");
}
