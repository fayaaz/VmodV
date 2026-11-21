//! Golden snapshot of the VmodV Output panel.
//!
//! The panel is where the output window's resolution and fullscreen state
//! are set: the preset buttons, the custom width/height fields, the
//! fullscreen checkbox and a readout of the current size. The fixture is
//! deterministic (fixed resolution), so the image is reproducible on every
//! run.
//!
//! Run `cargo test -p modv-ui --test output_snapshot`; the first run writes
//! `tests/snapshots/output_panel.png` and the next compares against it.

use egui_kittest::Harness;
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineHealth, EngineSnapshot, UiState};

#[test]
fn output_panel_snapshot() {
    let snapshot = EngineSnapshot {
        fps: 60.0,
        frame_ms: 16.6,
        resolution: [1920, 1080],
        scene_rows: vec!["Background".to_string(), "Effects".to_string()],
        health: EngineHealth {
            adapter: "RADV GFX1201".to_string(),
            backend: "Vulkan".to_string(),
            frames_presented: 120,
            output_open: true,
            output_fullscreen: false,
            audio_active: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Output;
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        );
    modv_ui::theme::apply(&harness.ctx);
    harness.run();
    harness.snapshot("output_panel");
}
