//! The File menu's VmodV actions and shortcuts.
//!
//! Menu clicks only queue commands; the VmodV app owns the paths and the
//! dialogs. Testing the queue here proves the menu wiring without a real file
//! dialog (the app-side `FilePicker` seam is exercised separately).

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::file_dialog::FileDialogAction;
use modv_ui::model::ProjectView;
use modv_ui::{EngineSnapshot, UiCommand, UiState};

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

fn open_file_menu(mut harness: Harness<'static, UiState>) -> Harness<'static, UiState> {
    harness.run();
    harness.get_by_label("File").click();
    harness.run();
    harness
}

#[test]
fn file_menu_queues_every_vmodv_dialog_action() {
    let mut harness = open_file_menu(harness(EngineSnapshot::default(), UiState::new()));

    harness.get_by_label("Open preset…").click();
    harness.run();
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Open Scene…").click();
    harness.run();
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Import playlist…").click();
    harness.run();
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Save preset as…").click();
    harness.run();
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export playlist…").click();
    harness.run();
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Choose extra ISF folder…").click();
    harness.run();
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Open media folder").click();
    harness.run();
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Open logs").click();
    harness.run();

    assert_eq!(
        harness.state().commands,
        vec![
            UiCommand::ShowFileDialog(FileDialogAction::OpenPreset),
            UiCommand::ShowFileDialog(FileDialogAction::OpenScene),
            UiCommand::ShowFileDialog(FileDialogAction::ImportPlaylist),
            UiCommand::ShowFileDialog(FileDialogAction::SavePresetAs),
            UiCommand::ShowFileDialog(FileDialogAction::ExportPlaylist),
            UiCommand::ShowFileDialog(FileDialogAction::AddIsfFolder),
            UiCommand::OpenMediaFolder,
            UiCommand::OpenLogsFolder,
        ]
    );
}

#[test]
fn the_switch_profile_submenu_queues_the_chosen_profile() {
    let snapshot = EngineSnapshot {
        project: ProjectView {
            active_profile: "default".to_string(),
            profiles: vec!["default".to_string(), "live".to_string()],
        },
        ..Default::default()
    };
    let mut harness = open_file_menu(harness(snapshot, UiState::new()));

    harness.get_by_label("Switch profile").click();
    harness.run();
    harness.get_by_label("live").click();
    harness.run();

    assert_eq!(
        harness.state().commands,
        vec![UiCommand::SwitchProfile("live".to_string())]
    );
}

#[test]
fn new_project_profile_opens_the_name_dialog() {
    let mut harness = open_file_menu(harness(EngineSnapshot::default(), UiState::new()));

    harness.get_by_label("New project profile…").click();
    harness.run();

    assert!(harness.state().show_profile_dialog);
}

#[test]
fn the_profile_dialog_queues_the_typed_name() {
    let mut harness = harness(EngineSnapshot::default(), UiState::new());
    harness.run();
    harness.state_mut().open_profile_dialog();
    harness.state_mut().profile_name = " live ".to_string();
    harness.run();

    harness.get_by_label("Create").click();
    harness.run();

    assert_eq!(
        harness.state().commands,
        vec![UiCommand::CreateProfile("live".to_string())]
    );
    assert!(!harness.state().show_profile_dialog);
}

#[test]
fn the_open_and_save_shortcuts_queue_the_same_commands_as_the_menu() {
    let mut harness = harness(EngineSnapshot::default(), UiState::new());
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::O);
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::O);
    harness.run();

    assert_eq!(
        harness.state().commands,
        vec![
            UiCommand::ShowFileDialog(FileDialogAction::OpenPreset),
            UiCommand::ShowFileDialog(FileDialogAction::OpenScene),
        ]
    );
}
