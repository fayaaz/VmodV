//! Golden snapshot of the native Quality panel.
//!
//! The panel is where the internal rendering resolution preference lives:
//! the three Full/Balanced/Performance presets and a readout of the current
//! render size. The fixture is deterministic (fixed resolution), so the image
//! is reproducible on every run.
//!
//! Run `cargo test -p modv-ui --test quality_snapshot`; the first run writes
//! `tests/snapshots/quality_panel.png` and the next compares against it.

use egui_kittest::Harness;
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineHealth, EngineSnapshot, UiState};

#[test]
fn quality_panel_snapshot() {
    let snapshot = EngineSnapshot {
        fps: 60.0,
        frame_ms: 16.6,
        resolution: [1280, 720],
        scene_rows: vec!["Background".to_string(), "Effects".to_string()],
        health: EngineHealth {
            adapter: "RADV GFX1201".to_string(),
            backend: "Vulkan".to_string(),
            frames_presented: 120,
            output_open: true,
            audio_active: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut state = UiState::new();
    state.render_scale = 0.5;
    state.side_panel_tab = SidePanelTab::Quality;
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
    harness.snapshot("quality_panel");
}
