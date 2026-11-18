//! The native in-app file picker.
//!
//! The native shell cannot assume a desktop portal is running (the Docker
//! `make native` session forwards Wayland/X11 and PulseAudio but not the D-Bus
//! session, and the Rust toolchain image has no GTK headers for `rfd`'s default
//! backend). A compact egui picker reads directories with `std::fs` instead:
//! it is only UI state, so it never blocks the frame loop, and it is fully
//! testable in kittest.
//!
//! A menu click only queues [`crate::UiCommand::ShowFileDialog`]; the native
//! app resolves the starting directory from the active media profile and calls
//! [`crate::UiState::open_file_dialog`]. Confirming queues one
//! [`crate::UiCommand::FileChosen`] with the action and the validated path.

use std::path::{Path, PathBuf};

use crate::state::{UiCommand, UiState};

/// What a file dialog is being opened for.
///
/// The variants mirror the Electron File menu: open a preset, a Scene or a
/// Scenes playlist, save a preset, export/import a playlist, or add a user ISF
/// search folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialogAction {
    OpenPreset,
    OpenScene,
    ImportPlaylist,
    SavePresetAs,
    ExportPlaylist,
    AddIsfFolder,
}

/// Whether the picker returns a file, a save target or a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialogMode {
    OpenFile,
    SaveFile,
    ChooseFolder,
}

impl FileDialogAction {
    /// Every action in menu order.
    pub const ALL: [Self; 6] = [
        Self::OpenPreset,
        Self::OpenScene,
        Self::ImportPlaylist,
        Self::SavePresetAs,
        Self::ExportPlaylist,
        Self::AddIsfFolder,
    ];

    /// The window title and the menu label.
    pub fn title(self) -> &'static str {
        match self {
            Self::OpenPreset => "Open preset",
            Self::OpenScene => "Open Scene",
            Self::ImportPlaylist => "Import playlist",
            Self::SavePresetAs => "Save preset as",
            Self::ExportPlaylist => "Export playlist",
            Self::AddIsfFolder => "Choose extra ISF folder",
        }
    }

    pub fn mode(self) -> FileDialogMode {
        match self {
            Self::OpenPreset | Self::OpenScene | Self::ImportPlaylist => FileDialogMode::OpenFile,
            Self::SavePresetAs | Self::ExportPlaylist => FileDialogMode::SaveFile,
            Self::AddIsfFolder => FileDialogMode::ChooseFolder,
        }
    }

    /// The extensions the picker lists. Empty for a folder picker.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::OpenPreset
            | Self::OpenScene
            | Self::ImportPlaylist
            | Self::SavePresetAs
            | Self::ExportPlaylist => &["json"],
            Self::AddIsfFolder => &[],
        }
    }

    /// The prefilled save name, if any.
    pub fn suggested_name(self) -> &'static str {
        match self {
            Self::SavePresetAs => "preset.json",
            Self::ExportPlaylist => "scenes-playlist.json",
            _ => "",
        }
    }

    /// The active media-profile folder the picker starts in.
    pub fn start_folder(self) -> Option<&'static str> {
        match self {
            Self::OpenPreset | Self::SavePresetAs => Some("preset"),
            Self::OpenScene | Self::ImportPlaylist | Self::ExportPlaylist => Some("scene"),
            Self::AddIsfFolder => Some("isf"),
        }
    }

    /// The command the native app runs once the user confirms `path`.
    pub fn command(self, path: PathBuf) -> UiCommand {
        UiCommand::FileChosen { action: self, path }
    }
}

/// One row in the picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntryView {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// The live state of the picker window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDialogState {
    pub action: FileDialogAction,
    pub directory: PathBuf,
    /// Editable copy of `directory` for the path bar.
    pub directory_draft: String,
    pub file_name: String,
    pub entries: Vec<FileEntryView>,
    pub error: Option<String>,
    pub show_hidden: bool,
}

impl FileDialogState {
    /// Open a picker at `start_dir`, falling back to its parent, the current
    /// directory or the home directory when the profile folder is missing.
    pub fn open(action: FileDialogAction, start_dir: impl Into<PathBuf>) -> Self {
        let requested = start_dir.into();
        let directory = fallback_directory(&requested);
        let mut dialog = Self {
            action,
            directory_draft: directory.display().to_string(),
            directory,
            file_name: action.suggested_name().to_string(),
            entries: Vec::new(),
            error: None,
            show_hidden: false,
        };
        dialog.refresh();
        dialog
    }

    /// Re-read `directory`.
    pub fn refresh(&mut self) {
        match read_entries(&self.directory, self.action.extensions(), self.show_hidden) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(error) => {
                self.entries.clear();
                self.error = Some(error);
            }
        }
        self.directory_draft = self.directory.display().to_string();
    }

    /// Navigate to `dir`. Returns whether it was a readable directory.
    pub fn navigate(&mut self, dir: &Path) -> bool {
        if !dir.is_dir() {
            self.error = Some(format!("{} is not a folder", dir.display()));
            return false;
        }
        self.directory = dir.to_path_buf();
        self.refresh();
        self.error.is_none()
    }

    /// Navigate to the parent directory, when there is one.
    pub fn up(&mut self) {
        let parent = self.directory.parent().map(std::path::Path::to_path_buf);
        if let Some(parent) = parent {
            self.navigate(&parent);
        }
    }

    /// Select `entry`: folders navigate, files fill the name field.
    pub fn select(&mut self, entry: &FileEntryView) {
        if entry.is_dir {
            self.navigate(&entry.path);
        } else {
            self.file_name = entry.name.clone();
        }
    }

    /// The path a confirmation would return, or `None` while the selection is
    /// unusable (empty name, missing file, wrong extension).
    pub fn confirm_path(&self) -> Option<PathBuf> {
        match self.action.mode() {
            FileDialogMode::ChooseFolder => self.directory.is_dir().then(|| self.directory.clone()),
            FileDialogMode::OpenFile => {
                let path = self.directory.join(self.file_name.trim());
                (path.is_file() && is_dialog_extension_path(&path, self.action.extensions()))
                    .then_some(path)
            }
            FileDialogMode::SaveFile => {
                let name = self.file_name.trim();
                if name.is_empty() {
                    return None;
                }
                Some(ensure_extension(
                    self.directory.join(name),
                    self.action.extensions().first().copied().unwrap_or("json"),
                ))
            }
        }
    }

    /// Why [`Self::confirm_path`] refused the current selection.
    pub fn confirm_error(&self) -> String {
        match self.action.mode() {
            FileDialogMode::ChooseFolder => {
                format!("{} is not a readable folder", self.directory.display())
            }
            FileDialogMode::OpenFile => {
                if self.file_name.trim().is_empty() {
                    "Choose a file".to_string()
                } else {
                    format!(
                        "{} is not a {} file",
                        self.file_name.trim(),
                        self.action
                            .extensions()
                            .first()
                            .copied()
                            .unwrap_or("supported")
                    )
                }
            }
            FileDialogMode::SaveFile => "Enter a file name".to_string(),
        }
    }
}

/// Whether `name` ends in one of `extensions` (case-insensitive).
pub fn is_dialog_extension(name: &str, extensions: &[&str]) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

fn is_dialog_extension_path(path: &Path, extensions: &[&str]) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| is_dialog_extension(name, extensions))
}

/// Append `extension` unless the path already ends in it.
///
/// A different extension is kept and the wanted one appended
/// (`preset.txt` -> `preset.txt.json`) rather than replacing what the user
/// typed.
pub fn ensure_extension(path: PathBuf, extension: &str) -> PathBuf {
    let already = path
        .extension()
        .and_then(|existing| existing.to_str())
        .is_some_and(|existing| existing.eq_ignore_ascii_case(extension));
    if already {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}

fn fallback_directory(requested: &Path) -> PathBuf {
    if requested.is_dir() {
        return requested.to_path_buf();
    }
    if let Some(parent) = requested.parent()
        && parent.is_dir()
    {
        return parent.to_path_buf();
    }
    if let Ok(cwd) = std::env::current_dir()
        && cwd.is_dir()
    {
        return cwd;
    }
    home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn read_entries(
    directory: &Path,
    extensions: &[&str],
    show_hidden: bool,
) -> Result<Vec<FileEntryView>, String> {
    let read = std::fs::read_dir(directory)
        .map_err(|error| format!("Could not read {}: {error}", directory.display()))?;
    let mut entries = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && !show_hidden {
            continue;
        }
        let path = entry.path();
        // `metadata()` follows symlinks, so a linked folder still navigates.
        let is_dir = entry.metadata().map(|meta| meta.is_dir()).unwrap_or(false);
        if !is_dir && !is_dialog_extension(&name, extensions) {
            continue;
        }
        entries.push(FileEntryView { name, path, is_dir });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

/// Draw the picker window and queue its command on confirmation.
pub fn file_dialog_window(ctx: &egui::Context, state: &mut UiState) {
    let Some(mut dialog) = state.file_dialog.take() else {
        return;
    };
    let mut open = true;
    let mut confirm = false;
    let mut cancel = false;

    egui::Window::new(dialog.action.title())
        .id(egui::Id::unique("modv-file-dialog"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([640.0, 480.0])
        .min_width(460.0)
        .min_height(320.0)
        .show(ctx, |ui| {
            dialog.path_row(ui);
            ui.separator();
            dialog.entries_list(ui, &mut confirm);
            ui.separator();
            dialog.footer(ui, &mut confirm, &mut cancel);
        });

    if confirm {
        match dialog.confirm_path() {
            Some(path) => {
                state.queue(dialog.action.command(path));
                return;
            }
            None => dialog.error = Some(dialog.confirm_error()),
        }
    }
    if open && !cancel {
        state.file_dialog = Some(dialog);
    }
}

impl FileDialogState {
    fn path_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.directory.parent().is_some(), egui::Button::new("Up"))
                .clicked()
            {
                self.up();
            }
            if ui.button("Home").clicked()
                && let Some(home) = home_dir()
            {
                self.navigate(&home);
            }
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.directory_draft)
                    .desired_width(320.0)
                    .hint_text("Folder"),
            );
            let entered =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let typed = self.directory_draft.trim().to_string();
            if entered || ui.button("Go").clicked() {
                self.navigate(Path::new(&typed));
            }
        });
        ui.checkbox(&mut self.show_hidden, "Show hidden files");
        if let Some(error) = &self.error {
            ui.colored_label(crate::theme::DANGER, error);
        }
    }

    fn entries_list(&mut self, ui: &mut egui::Ui, confirm: &mut bool) {
        let mut navigate = None;
        let mut select = None;
        let mut confirm_now = false;
        egui::ScrollArea::vertical()
            .max_height(280.0)
            .min_scrolled_height(180.0)
            .show(ui, |ui| {
                if self.entries.is_empty() {
                    ui.label(
                        egui::RichText::new("No matching files here.")
                            .size(12.0)
                            .color(crate::theme::MUTED),
                    );
                }
                let selected = self.file_name.clone();
                for entry in &self.entries {
                    let label = if entry.is_dir {
                        format!("{}/", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    let is_selected = !entry.is_dir && entry.name == selected;
                    let response = ui.selectable_label(is_selected, label);
                    if response.clicked() {
                        if entry.is_dir {
                            navigate = Some(entry.path.clone());
                        } else {
                            select = Some(entry.clone());
                        }
                    }
                    if response.double_clicked() && !entry.is_dir {
                        select = Some(entry.clone());
                        confirm_now = true;
                    }
                }
            });
        if let Some(path) = navigate {
            self.navigate(&path);
        }
        if let Some(entry) = select {
            self.select(&entry);
        }
        if confirm_now {
            *confirm = true;
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui, confirm: &mut bool, cancel: &mut bool) {
        ui.horizontal(|ui| {
            match self.action.mode() {
                FileDialogMode::ChooseFolder => {
                    ui.label(
                        egui::RichText::new(self.directory.display().to_string())
                            .monospace()
                            .size(11.0)
                            .color(crate::theme::MUTED),
                    );
                }
                FileDialogMode::OpenFile | FileDialogMode::SaveFile => {
                    ui.label("File name");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.file_name)
                            .desired_width(260.0)
                            .hint_text(self.action.suggested_name()),
                    );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    *cancel = true;
                }
                let confirm_label = match self.action.mode() {
                    FileDialogMode::OpenFile => "Open",
                    FileDialogMode::SaveFile => "Save",
                    FileDialogMode::ChooseFolder => "Choose this folder",
                };
                if ui.button(confirm_label).clicked() {
                    *confirm = true;
                }
            });
        });
    }
}

/// Draw the "New project profile" modal.
pub fn profile_dialog_window(ctx: &egui::Context, state: &mut UiState) {
    if !state.show_profile_dialog {
        return;
    }
    let mut open = true;
    let mut create = false;
    let mut cancel = false;
    egui::Window::new("New project profile")
        .id(egui::Id::unique("modv-profile-dialog"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .show(ctx, |ui| {
            ui.label("Profile name");
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.profile_name)
                    .desired_width(300.0)
                    .hint_text("e.g. live"),
            );
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                create = true;
            }
            if let Some(error) = &state.profile_error {
                ui.colored_label(crate::theme::DANGER, error);
            }
            ui.label(
                egui::RichText::new(
                    "Creates image, video, palette, preset, scene, module, plugin, \
                     plugin_data and isf folders under <userData>/media/<name>.",
                )
                .size(11.0)
                .color(crate::theme::MUTED),
            );
            ui.horizontal(|ui| {
                if ui.button("Create").clicked() {
                    create = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if create {
        state.queue_create_profile();
    }
    if !open || cancel {
        state.show_profile_dialog = false;
        state.profile_error = None;
    }
}
