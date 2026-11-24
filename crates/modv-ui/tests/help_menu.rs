//! The Help menu's user-facing actions.
//!
//! Menu clicks only queue commands; the VmodV app owns the paths and the
//! dialogs. Asserting the queue here proves the wiring without a real file
//! manager.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, UiCommand, UiState};

fn harness() -> Harness<'static, UiState> {
    let harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &EngineSnapshot::default(), state);
            },
            UiState::new(),
        );
    modv_ui::theme::apply(&harness.ctx);
    harness
}

#[test]
fn help_menu_open_logs_opens_the_logs_folder() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Help").click();
    harness.run();
    harness.get_by_label("Open logs folder").click();
    harness.run();

    assert_eq!(
        harness.state().commands,
        vec![UiCommand::OpenLogsFolder],
        "Help > Open logs folder must open <userData>/logs, not just flush"
    );
}
