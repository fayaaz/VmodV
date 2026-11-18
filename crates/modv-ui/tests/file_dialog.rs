//! The native in-app file picker and the File-menu dialog commands.
//!
//! The picker exists because the native binary cannot rely on a desktop
//! portal: `make native` runs in a container with no D-Bus session, and the
//! Docker Rust image has no GTK headers for `rfd`'s default backend. Reading
//! directories through `std::fs` keeps the dialog non-blocking (it is just
//! egui state) and testable in plain kittest.

use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::file_dialog::{
    FileDialogAction, FileDialogMode, FileDialogState, ensure_extension, is_dialog_extension,
};
use modv_ui::{EngineSnapshot, UiCommand, UiState};

fn temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn harness(state: UiState) -> Harness<'static, UiState> {
    let snapshot = EngineSnapshot::default();
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
fn open_dialog_lists_folders_and_only_matching_files() {
    let root = temp_dir("modv-file-dialog");
    std::fs::create_dir_all(root.join("nested")).expect("nested");
    std::fs::write(root.join("preset.json"), "{}").expect("json");
    std::fs::write(root.join("notes.txt"), "hello").expect("txt");

    let dialog = FileDialogState::open(FileDialogAction::OpenPreset, &root);
    let names: Vec<&str> = dialog
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert_eq!(
        names,
        vec!["nested", "preset.json"],
        "folders sort first and non-JSON files are filtered out"
    );
    assert!(dialog.entries[0].is_dir);
    assert!(!dialog.entries[1].is_dir);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn extension_helpers_match_and_append_case_insensitively() {
    assert!(is_dialog_extension("Preset.JSON", &["json"]));
    assert!(is_dialog_extension("shader.fs", &["fs", "vs"]));
    assert!(!is_dialog_extension("notes.txt", &["json"]));
    assert!(!is_dialog_extension("preset", &["json"]));

    assert_eq!(
        ensure_extension(PathBuf::from("/tmp/preset"), "json"),
        PathBuf::from("/tmp/preset.json")
    );
    assert_eq!(
        ensure_extension(PathBuf::from("/tmp/preset.JSON"), "json"),
        PathBuf::from("/tmp/preset.JSON"),
        "an existing extension is kept as typed"
    );
    assert_eq!(
        ensure_extension(PathBuf::from("/tmp/preset.txt"), "json"),
        PathBuf::from("/tmp/preset.txt.json"),
        "a wrong extension is not silently replaced"
    );
}

#[test]
fn dialog_metadata_matches_the_electron_filters() {
    assert_eq!(
        FileDialogAction::OpenPreset.mode(),
        FileDialogMode::OpenFile
    );
    assert_eq!(
        FileDialogAction::SavePresetAs.mode(),
        FileDialogMode::SaveFile
    );
    assert_eq!(
        FileDialogAction::AddIsfFolder.mode(),
        FileDialogMode::ChooseFolder
    );
    assert_eq!(
        FileDialogAction::ImportPlaylist.extensions(),
        &["json"],
        "playlists are JSON documents"
    );
    assert_eq!(FileDialogAction::OpenScene.title(), "Open Scene");
    assert_eq!(
        FileDialogAction::SavePresetAs.suggested_name(),
        "preset.json"
    );
    assert_eq!(
        FileDialogAction::ExportPlaylist.suggested_name(),
        "scenes-playlist.json"
    );
}

#[test]
fn choosing_a_file_queues_exactly_one_command() {
    let root = temp_dir("modv-file-dialog-confirm");
    std::fs::write(root.join("preset.json"), "{}").expect("json");

    let mut state = UiState::new();
    state.open_file_dialog(FileDialogAction::OpenPreset, &root);
    let mut harness = harness(state);
    harness.run();

    harness.get_by_label("preset.json").click();
    harness.run();
    harness.get_by_label("Open").click();
    harness.run();

    assert_eq!(
        harness.state().file_dialog,
        None,
        "confirm closes the dialog"
    );
    assert_eq!(
        harness.state().commands,
        vec![UiCommand::FileChosen {
            action: FileDialogAction::OpenPreset,
            path: root.join("preset.json"),
        }]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cancelling_queues_nothing() {
    let root = temp_dir("modv-file-dialog-cancel");
    let mut state = UiState::new();
    state.open_file_dialog(FileDialogAction::OpenPreset, &root);
    let mut harness = harness(state);
    harness.run();

    harness.get_by_label("Cancel").click();
    harness.run();

    assert_eq!(harness.state().file_dialog, None);
    assert!(harness.state().commands.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_folder_dialog_returns_the_open_directory() {
    let root = temp_dir("modv-file-dialog-folder");
    let mut state = UiState::new();
    state.open_file_dialog(FileDialogAction::AddIsfFolder, &root);
    let mut harness = harness(state);
    harness.run();

    harness.get_by_label("Choose this folder").click();
    harness.run();

    assert_eq!(
        harness.state().commands,
        vec![UiCommand::FileChosen {
            action: FileDialogAction::AddIsfFolder,
            path: root.clone(),
        }]
    );

    let _ = std::fs::remove_dir_all(root);
}
