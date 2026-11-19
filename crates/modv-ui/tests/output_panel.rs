//! Interaction tests for the native Output panel: what it renders and
//! which commands it queues.
//!
//! The panel is the user-facing way to set the output window's resolution
//! and toggle fullscreen. The fixture runs the real shell draw against a
//! fixed, deterministic snapshot.
//!
//! Run `cargo test -p modv-ui --test output_panel`.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineHealth, EngineSnapshot, UiCommand, UiState};

fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        fps: 60.0,
        frame_ms: 16.6,
        resolution: [1024, 768],
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
    }
}

fn harness() -> Harness<'static, UiState> {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Output;
    Harness::builder()
        .with_size([1440.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot(), state);
            },
            state,
        )
}

#[test]
fn the_output_tab_lists_every_preset_and_marks_the_current_one() {
    let mut harness = harness();
    harness.run();

    for label in ["960x540", "1280x720", "1920x1080", "2560x1440", "3840x2160"] {
        harness.get_by_label(label);
    }

    // 1024x768 is the current resolution in the fixture, so its readout
    // shows it.
    harness.get_by_label("Current: 1024x768");
    harness.get_by_label("Fullscreen (F)");

    let commands = harness.state_mut().take_commands();
    assert!(
        commands.is_empty(),
        "rendering must not dispatch: {commands:?}"
    );
}

#[test]
fn clicking_a_preset_queues_the_size_command() {
    let mut harness = harness();
    harness.run();

    harness.get_by_label("1920x1080").click();
    harness.run();

    let commands = harness.state_mut().take_commands();
    assert_eq!(
        commands,
        vec![UiCommand::SetOutputSize {
            width: 1920,
            height: 1080,
        }]
    );
}

#[test]
fn the_fullscreen_checkbox_queues_the_absolute_state() {
    let mut harness = harness();
    harness.run();

    harness.get_by_label("Fullscreen (F)").click();
    harness.run();

    let commands = harness.state_mut().take_commands();
    assert_eq!(commands, vec![UiCommand::SetOutputFullscreen(true)]);
}

#[test]
fn a_closed_output_window_offers_an_open_button() {
    let mut snapshot = snapshot();
    snapshot.health.output_open = false;
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
    harness.run();

    harness.get_by_label("The output window is closed.");
    harness.get_by_label("Open output window").click();
    harness.run();

    let commands = harness.state_mut().take_commands();
    assert_eq!(commands, vec![UiCommand::OpenOutputWindow]);
}
