//! The Edit menu's undo/redo actions and shortcuts.
//!
//! Menu clicks and key presses only queue engine actions; the shell owns the
//! Store history. Testing the queue here proves the UI wiring without needing
//! a live engine.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, UiState};

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

fn open_edit_menu(mut harness: Harness<'static, UiState>) -> Harness<'static, UiState> {
    harness.run();
    harness.get_by_label("Edit").click();
    harness.run();
    harness
}

#[test]
fn edit_menu_undo_and_redo_queue_history_actions() {
    let mut harness = open_edit_menu(harness(EngineSnapshot::default(), UiState::new()));

    harness.get_by_label("Undo").click();
    harness.run();
    assert_eq!(
        harness
            .state()
            .actions
            .first()
            .map(|action| action.action.as_str()),
        Some("history/undo"),
        "Edit > Undo must queue the history/undo action"
    );

    // The menu closes on a click; reopen it for the second item.
    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label("Redo").click();
    harness.run();
    assert_eq!(
        harness
            .state()
            .actions
            .get(1)
            .map(|action| action.action.as_str()),
        Some("history/redo"),
        "Edit > Redo must queue the history/redo action"
    );
}

#[test]
fn ctrl_z_and_ctrl_shift_z_queue_history_actions() {
    let mut harness = harness(EngineSnapshot::default(), UiState::new());
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Z);
    harness.run();

    let actions: Vec<_> = harness
        .state()
        .actions
        .iter()
        .map(|action| action.action.as_str())
        .collect();
    assert_eq!(
        actions,
        vec!["history/undo", "history/redo"],
        "Ctrl+Z and Ctrl+Shift+Z must queue the matching history actions"
    );
}
